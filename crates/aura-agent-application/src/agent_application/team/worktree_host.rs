//! The application as the team's worktree host: where a new worktree goes, the Git work through
//! the native Git writer, the hidden workspace each worktree is registered as, the processes
//! started in it, and its size on disk.
use super::*;
use anyhow::Context as _;
use aura_agent_execution::command_execution::process_policy;
use aura_agent_team::{
    AddedWorktree, DEFAULT_LOCATION_FOLDER, MergeStep, Worktree, WorktreeChange,
    WorktreeChangeKind, WorktreeHost, WorktreeMerge, WorktreePlan,
};
use aura_git_service::git::{
    AGENT_BRANCH_PREFIX, CARRY_MAX_BYTES, CARRY_MAX_FILES, ChangeKind, GitReader, GitWriter,
    MergeOutcome, WorktreeSpec,
};
use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Duration,
};

/// What a drive keeps free after a new worktree's files are written.
const DISK_RESERVE: u64 = 5 * 1024 * 1024 * 1024;
/// A worktree's size is counted again when the last count is older than this.
const SIZE_FRESH: Duration = Duration::from_secs(60);
/// The commit on a new worktree's branch that holds the workspace's uncommitted changes.
const TAKEN_ALONG: &str = "Aura：建工作副本时主工作区没提交的改动";

impl WorktreeHost for TeamApp {
    fn plan_worktree(
        &self,
        workspace: WorkspaceId,
        key: &str,
    ) -> impl Future<Output = anyhow::Result<WorktreePlan>> + Send {
        let app = Arc::clone(&self.0);
        let key = key.to_owned();
        async move {
            let (chosen, _) = app
                .store
                .read(StoreStep::operation("agents.worktree.location"), |db| {
                    aura_agent_team::worktree::location(db)
                })
                .await?;
            let root = workspace_root(&app, workspace)
                .map_err(|error| not_made("找不到主工作区的文件夹", &error))?;
            let location = location_folder(chosen.as_deref())?;
            let path = location.join(folder_name(&root, workspace)).join(&key);
            check_location(&app, &path)?;
            anyhow::ensure!(
                !inside(&path, &root),
                "agents.worktree_location: 工作副本的位置在这个工作区里面，请用户在 设置 → 存储 → 工作副本 换一个位置。"
            );
            tokio::task::spawn_blocking(move || {
                let reader =
                    GitReader::open(&root, Arc::new(AtomicBool::new(false))).map_err(|error| {
                        anyhow::anyhow!(
                            "agents.not_git: 这个工作区读不出 Git 仓库：{}",
                            aura_agent_team::cause_of(&error)
                        )
                    })?;
                let start = reader.head_commit().map_err(|_| {
                    anyhow::anyhow!("agents.no_commit: 工作区还没有任何提交，建不了工作副本。")
                })?;
                // The uncommitted files go along into the worktree.
                let carried = reader
                    .changed_from(&start)
                    .map_err(|error| not_made("读主工作区没提交的改动时出错", &error))?;
                let carried_bytes = reader
                    .files_size(&carried)
                    .map_err(|error| not_made("算没提交的改动有多大时出错", &error))?;
                check_carry(carried.len(), carried_bytes)?;
                let needed = reader
                    .checkout_size(&start)
                    .map_err(|error| not_made("算工作副本要占多少空间时出错", &error))?
                    .saturating_add(carried_bytes);
                check_space(&path, needed)?;
                Ok(WorktreePlan {
                    path: path.display().to_string(),
                    branch: format!("{AGENT_BRANCH_PREFIX}{key}"),
                    start,
                })
            })
            .await
            .map_err(|error| not_made("准备工作副本的任务中断了", &error.into()))?
        }
    }

    fn add_worktree(
        &self,
        workspace: WorkspaceId,
        plan: &WorktreePlan,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<AddedWorktree>> + Send {
        let app = Arc::clone(&self.0);
        let plan = plan.clone();
        async move {
            let root = workspace_root(&app, workspace)
                .map_err(|error| not_made("找不到主工作区的文件夹", &error))?;
            let spec = WorktreeSpec {
                path: PathBuf::from(&plan.path),
                branch: plan.branch.clone(),
                start: plan.start.clone(),
            };
            let (entry, base, carried) = tokio::task::spawn_blocking(move || {
                let cancel = Arc::new(AtomicBool::new(false));
                let main = GitReader::open(&root, Arc::clone(&cancel))
                    .map_err(|error| not_made("打开主工作区的 Git 仓库时出错", &error))?;
                let carried = main
                    .changed_from(&spec.start)
                    .map_err(|error| not_made("读主工作区没提交的改动时出错", &error))?;
                let entry = GitWriter::new(main, || {})
                    .add_worktree(&spec)
                    .map_err(|error| not_made("用 git worktree add 建工作副本时出错", &error))?;
                let own = GitReader::open(&entry.path, cancel)
                    .map_err(|error| not_made("打开新建的工作副本时出错", &error))?;
                let base = GitWriter::new(own, || {})
                    .take_along(&root, &carried, &spec.start, TAKEN_ALONG)
                    .map_err(|error| {
                        not_made("把主工作区没提交的改动带进工作副本时出错", &error)
                    })?;
                anyhow::Ok((entry, base, carried))
            })
            .await
            .map_err(|error| not_made("建工作副本的任务中断了", &error.into()))??;
            let parent = app
                .execution
                .persistence
                .workspace(workspace)
                .map_err(|error| not_made("读主工作区的记录时出错", &error.into()))?;
            let short = plan.branch.trim_start_matches(AGENT_BRANCH_PREFIX).to_owned();
            let command = aura_workspace_domain::WorkspaceCreateCommand {
                selected_root: entry.path.display().to_string(),
                display_name: format!("{} · {short}", parent.display_name),
            };
            let runtime = Arc::clone(&app.execution.workspace);
            let created = tokio::task::spawn_blocking(move || {
                runtime.create_worktree(command, workspace, CorrelationId::new())
            })
            .await
            .map_err(|error| not_made("登记工作副本的任务中断了", &error.into()))?
            .map_err(|error| not_made("把工作副本登记成 Aura 的工作区时出错", &error.into()))?;
            let carried = if base == plan.start { Vec::new() } else { carried };
            Ok(AddedWorktree { workspace_id: created.workspace_id, base, carried })
        }
    }

    fn worktree_changes(
        &self,
        worktree: &Worktree,
    ) -> impl Future<Output = anyhow::Result<Vec<WorktreeChange>>> + Send {
        let path = PathBuf::from(&worktree.details.path);
        let base = worktree.details.base.clone();
        async move {
            tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<WorktreeChange>> {
                let reader = GitReader::open(&path, Arc::new(AtomicBool::new(false)))?;
                Ok(reader.changes_since(&base)?.into_iter().map(change).collect())
            })
            .await
            .map_err(anyhow::Error::from)
            .and_then(|read| read)
            .map_err(|error| {
                anyhow::anyhow!(
                    "agents.worktree_failed: 读工作副本的改动时出错：{}。什么都没改，可以稍后再看。",
                    aura_agent_team::cause_of(&error)
                )
            })
        }
    }

    fn merge_worktree(
        &self,
        worktree: &Worktree,
        message: &str,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<WorktreeMerge>> + Send {
        let app = Arc::clone(&self.0);
        let worktree = worktree.clone();
        let message = message.to_owned();
        async move {
            // Each step's failure names it: until the changes are brought in, the workspace is
            // as it was.
            let root =
                workspace_root(&app, worktree.parent_workspace_id).context(MergeStep::Locate)?;
            tokio::task::spawn_blocking(move || {
                let cancel = Arc::new(AtomicBool::new(false));
                let files = commit_own(&worktree, &message, &cancel).context(MergeStep::Commit)?;
                let main = GitReader::open(&root, cancel).context(MergeStep::BringIn)?;
                let details = &worktree.details;
                let outcome = GitWriter::new(main, || {})
                    .bring_in(&details.branch, &details.base, &message)
                    .context(MergeStep::BringIn)?;
                Ok(match outcome {
                    MergeOutcome::Merged { head } => WorktreeMerge::Merged { head, files },
                    MergeOutcome::Applied { files } => WorktreeMerge::Applied { files },
                    MergeOutcome::UpToDate => WorktreeMerge::UpToDate,
                    MergeOutcome::Conflicted { files } => WorktreeMerge::Conflicted { files },
                    MergeOutcome::Clashed { files } => WorktreeMerge::Clashed { files },
                })
            })
            .await
            .context("合回的任务中断了")?
        }
    }

    fn remove_worktree(
        &self,
        worktree: &Worktree,
        _command: CommandInvocationId,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        let app = Arc::clone(&self.0);
        let worktree = worktree.clone();
        async move {
            // Its processes hold files in it.
            super::host::stop_processes(&app, worktree.session_id)
                .await
                .context("停不掉它的命令启动的进程")?;
            process_policy::remove_temp_directory(worktree.session_id);
            app.execution.close_model_page(worktree.session_id).await;
            let root = workspace_root(&app, worktree.parent_workspace_id)
                .context("找不到主工作区的文件夹")?;
            let runtime = Arc::clone(&app.execution.workspace);
            let path = PathBuf::from(&worktree.details.path);
            let sizes = Arc::clone(&app.execution.team);
            tokio::task::spawn_blocking(move || {
                // Its watcher holds the folder: the workspace is archived first.
                runtime
                    .retire_worktree(worktree.workspace_id, CorrelationId::new())
                    .context("关不掉工作副本对应的工作区")?;
                let reader = GitReader::open(&root, Arc::new(AtomicBool::new(false)))
                    .context("打开主工作区的 Git 仓库时出错")?;
                GitWriter::new(reader, || {})
                    .remove_worktree(&path, &worktree.details.branch)
                    .context("Git 没能删掉工作副本和它的分支")?;
                sizes.sizes().forget(&path);
                Ok(())
            })
            .await
            .context("删工作副本的任务中断了")?
        }
    }

    fn worktree_size(&self, worktree: &Worktree) -> Option<u64> {
        let path = PathBuf::from(&worktree.details.path);
        let (bytes, due) = self.0.execution.team.sizes().take(&path, SIZE_FRESH);
        if due {
            let team = Arc::clone(&self.0.execution.team);
            match tokio::runtime::Handle::try_current() {
                Ok(runtime) => {
                    runtime.spawn_blocking(move || {
                        let counted = folder_size(&path);
                        team.sizes().counted(&path, counted);
                    });
                }
                Err(_) => self.0.execution.team.sizes().counted(&path, bytes),
            }
        }
        bytes
    }
}

/// The folder new worktrees go under: the one chosen, or `AuraWorktrees` in the user's profile.
pub(super) fn location_folder(chosen: Option<&str>) -> anyhow::Result<PathBuf> {
    if let Some(chosen) = chosen {
        return Ok(PathBuf::from(chosen));
    }
    let profile = std::env::var_os("USERPROFILE").ok_or_else(|| {
        anyhow::anyhow!(
            "agents.worktree_location: 找不到用户文件夹，不知道把工作副本放在哪；请用户在 设置 → 存储 → 工作副本 选一个位置。"
        )
    })?;
    Ok(PathBuf::from(profile).join(DEFAULT_LOCATION_FOLDER))
}

/// A worktree that could not be made: the step that failed and why. No sub-agent was started.
fn not_made(step: &str, error: &anyhow::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "agents.worktree_failed: 工作副本没建成：{step}（{}）。子 Agent 没有开始干活；可以处理好原因再派，或者不写 worktree，让它在这个工作区里干活。",
        aura_agent_team::cause_of(error)
    )
}

/// Commits what the worktree has not committed yet; the files it changed since it started.
fn commit_own(
    worktree: &Worktree,
    message: &str,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<Vec<String>> {
    let own = GitReader::open(Path::new(&worktree.details.path), Arc::clone(cancel))?;
    let files =
        own.changes_since(&worktree.details.base)?.into_iter().map(|change| change.path).collect();
    GitWriter::new(own, || {}).commit_all(message)?;
    Ok(files)
}

/// Refuses a worktree location in Aura's own data or not written in full.
pub(super) fn check_location(app: &AgentApplication, path: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(path.is_absolute(), "agents.worktree_location: 工作副本的位置要写完整路径。");
    anyhow::ensure!(
        !app.execution.control_resources.in_aura_data(path),
        "agents.worktree_location: 工作副本不能放在 Aura 的数据位置里，请用户在 设置 → 存储 → 工作副本 换一个位置。"
    );
    Ok(())
}

/// Free space on the drive `path` would be on.
pub(super) fn free_space(path: &Path) -> Option<u64> {
    path.ancestors()
        .find(|folder| folder.is_dir())
        .and_then(|folder| aura_windows_platform::free_disk_space(folder).ok())
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// Refuses a worktree whose workspace has more uncommitted files than one takes along.
fn check_carry(files: usize, bytes: u64) -> anyhow::Result<()> {
    anyhow::ensure!(
        files <= CARRY_MAX_FILES && bytes <= CARRY_MAX_BYTES,
        "agents.carry_too_large: 工作区没提交的改动有 {files} 个文件、约 {:.1} GB，太多，带不进工作副本。请用户先提交，或把不需要的文件加进 .gitignore。",
        gib(bytes)
    );
    Ok(())
}

fn check_space(path: &Path, needed: u64) -> anyhow::Result<()> {
    let Some(free) = free_space(path) else { return Ok(()) };
    anyhow::ensure!(
        free >= needed.saturating_add(DISK_RESERVE),
        "agents.disk_low: 工作副本所在的盘只剩 {:.1} GB，放下工作副本（约 {:.1} GB）后不到 {:.0} GB。请用户腾出空间，或在 设置 → 存储 → 工作副本 换到别的盘。",
        gib(free),
        gib(needed),
        gib(DISK_RESERVE)
    );
    Ok(())
}

fn workspace_root(app: &AgentApplication, workspace: WorkspaceId) -> anyhow::Result<PathBuf> {
    let resolver = app.execution.workspace.resolver(workspace)?;
    resolver.verify_root_identity()?;
    Ok(resolver
        .resolve_existing(&aura_domain_common::WorkspacePath::parse(
            aura_domain_common::WorkspacePath::ROOT,
        )?)?
        .absolute_path)
}

/// The folder a workspace's worktrees go in: its own folder's name in letters, digits, `-`, `_`
/// and `.`, or one made from its id.
fn folder_name(root: &Path, workspace: WorkspaceId) -> String {
    let name: String = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '-' })
        .take(40)
        .collect();
    let name = name.trim_matches(|c| c == '-' || c == '.').to_owned();
    if name.is_empty() {
        let id = workspace.to_string();
        format!("workspace-{}", &id[id.len().saturating_sub(8)..])
    } else {
        name
    }
}

/// Whether `path` is `folder` or inside it, by their written form.
fn inside(path: &Path, folder: &Path) -> bool {
    let text = |path: &Path| {
        let text = path.to_string_lossy().replace('/', "\\");
        format!(
            "{}\\",
            text.strip_prefix(r"\\?\").unwrap_or(&text).trim_end_matches('\\').to_lowercase()
        )
    };
    text(path).starts_with(&text(folder))
}

fn change(change: aura_git_service::git::FileChange) -> WorktreeChange {
    WorktreeChange {
        path: change.path,
        kind: match change.kind {
            ChangeKind::Added => WorktreeChangeKind::Added,
            ChangeKind::Modified => WorktreeChangeKind::Modified,
            ChangeKind::Deleted => WorktreeChangeKind::Deleted,
        },
        added: change.added,
        removed: change.removed,
    }
}

/// Bytes of every file under `folder`; links are not followed.
pub(super) fn folder_size(folder: &Path) -> Option<u64> {
    let mut total = 0_u64;
    let mut pending = vec![folder.to_path_buf()];
    while let Some(next) = pending.pop() {
        let entries = match std::fs::read_dir(&next) {
            Ok(entries) => entries,
            Err(_) if next != folder => continue,
            Err(_) => return None,
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.metadata() else { continue };
            if metadata.is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    Some(total)
}
