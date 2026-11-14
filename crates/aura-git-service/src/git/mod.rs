//! Native Git reads over the approved repository and config scope. No private snapshots.
mod config;
mod envelope;
pub use config::DETAIL_LABEL;
pub use envelope::envelope;
mod cancellation;
pub use cancellation::{CancelOnDrop, GitCancelled, StopUnconfirmed};
mod attributes;
mod carry;
mod diff;
mod log;
mod remotes;
mod review;
pub mod runtime;
mod status;
mod untracked;
mod worktree;
mod writes;
use anyhow::{Result, ensure};
use aura_backend_windows_user::git::{GitProcessResult, GitRunner};
use aura_workspace_domain::{GitReadInputV1, GitReadResultV1};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
pub use worktree::{
    AGENT_BRANCH_PREFIX, ChangeKind, FileChange, MergeOutcome, WorktreeEntry, WorktreeSpec,
};
pub use writes::{CARRY_MAX_BYTES, CARRY_MAX_FILES, GitWriteInterrupted, GitWriter};

pub struct GitReader {
    pub(super) root: PathBuf,
    pub(super) runner: GitRunner,
    pub(super) config: Vec<String>,
    programs: config::DisabledPrograms,
    directories: Vec<aura_windows_platform::GitReadDirectory>,
    /// The Git directory and the common one, as the layout named them; both are held above.
    git_dir: PathBuf,
    common_dir: PathBuf,
    pub(super) cancel: Arc<AtomicBool>,
    deadline: std::time::Instant,
    /// The Git processes this reader started.
    #[cfg(test)]
    processes: std::sync::atomic::AtomicUsize,
}
impl GitReader {
    pub fn open(root: &Path, cancel: Arc<AtomicBool>) -> Result<Self> {
        let detected = GitRunner::detect().map_err(|error| {
            anyhow::anyhow!(
                "git.detection_failed: 无法检测本机 Git，这次 Git 操作没有执行。请在设置中重新检测 Git 后再试。{DETAIL_LABEL}{}",
                config::detail(&error.to_string())
            )
        })?;
        let runner = detected.runner.ok_or_else(|| {
            anyhow::anyhow!(
                "git.{}: {}，这次 Git 操作没有执行。请安装或升级本机 Git（2.40 以上）后在设置中重新检测；在此之前请不用 Git 继续。",
                detected.state,
                runtime::unavailable_message(detected.state)
            )
        })?;
        Self::with_runner(root, runner, cancel)
    }
    pub(super) fn with_runner(
        root: &Path,
        runner: GitRunner,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        let mut value = Self {
            root: root.to_owned(),
            runner,
            config: vec![],
            programs: Default::default(),
            directories: vec![
                aura_windows_platform::GitReadDirectory::open(root).map_err(directory_error)?,
            ],
            git_dir: PathBuf::new(),
            common_dir: PathBuf::new(),
            cancel,
            deadline: std::time::Instant::now() + Duration::from_secs(30),
            #[cfg(test)]
            processes: Default::default(),
        };
        value.verify_layout()?;
        (value.config, value.programs) = config::read_configuration(&value)?;
        Ok(value)
    }
    fn verify_layout(&mut self) -> Result<()> {
        // One rev-parse names both Git directories and the worktree (--path-format=absolute is in
        // 2.31; these were four processes). A bare repository has no worktree to label and
        // answers that with an error; the two questions below then follow.
        let directories =
            ["rev-parse", "--path-format=absolute", "--absolute-git-dir", "--git-common-dir"];
        let mut all = directories.map(str::to_owned).to_vec();
        all.push("--show-toplevel".into());
        let first = self.raw(&all)?;
        let layout = || config::unreadable_output("layout_format", "仓库目录");
        let output = if first.exit_code == Some(0) {
            String::from_utf8(first.stdout).map_err(|_| layout())?
        } else {
            let bare = self.command(&["rev-parse", "--is-bare-repository"])?;
            ensure!(bare == b"true\n", "{}", config::failure(&first.stderr));
            String::from_utf8(self.command(&directories)?).map_err(|_| layout())?
        };
        let mut paths = output.lines().map(PathBuf::from);
        let (Some(git_dir), Some(common_dir)) = (paths.next(), paths.next()) else {
            return Err(layout());
        };
        if let Some(worktree) = paths.next() {
            self.directories.push(
                aura_windows_platform::GitReadDirectory::open(&worktree)
                    .map_err(directory_error)?,
            );
        }
        for path in [&git_dir, &common_dir] {
            self.directories.push(
                aura_windows_platform::GitReadDirectory::open(path).map_err(directory_error)?,
            );
        }
        (self.git_dir, self.common_dir) = (git_dir, common_dir);
        Ok(())
    }
    pub fn read(&self, input: &GitReadInputV1) -> Result<GitReadResultV1> {
        Ok(match input {
            GitReadInputV1::Status(input) => GitReadResultV1::Status(self.status(input)?),
            GitReadInputV1::Diff(input) => GitReadResultV1::Diff(self.diff(input)?),
            GitReadInputV1::Log(input) => GitReadResultV1::Log(self.log(input)?),
            GitReadInputV1::UntrackedDiff(input) => {
                GitReadResultV1::Diff(self.untracked_diff(input)?)
            }
            GitReadInputV1::ReviewDiff(input) => GitReadResultV1::Diff(self.review_diff(input)?),
        })
    }
    pub fn repository_identity(&self) -> Result<aura_windows_platform::NativePathInfo> {
        // The common Git directory the layout named, held since.
        Ok(aura_windows_platform::inspect_existing_path(&self.common_dir, false)?)
    }
    pub fn facts(&self) -> Result<serde_json::Value> {
        let identity = self.repository_identity()?.identity;
        Ok(
            serde_json::json!({"repository":{"volume":identity.volume_serial_number.to_string(),"file_id":identity.file_id},
            "image":self.runner.image_binding(), "config":aura_domain_common::jcs::canonical_sha256(&self.config)?}),
        )
    }
    fn raw(&self, args: &[String]) -> Result<GitProcessResult> {
        self.execute(args, &self.config)
    }
    fn execute(&self, args: &[String], configuration: &[String]) -> Result<GitProcessResult> {
        for directory in &self.directories {
            directory.recheck().map_err(directory_error)?;
        }
        // All global options below predate 2.40. Command data is always a separate argv element.
        let mut command =
            vec!["--no-pager".into(), "--no-optional-locks".into(), "--literal-pathspecs".into()];
        command.extend_from_slice(configuration);
        for value in [
            "color.ui=false",
            "core.quotePath=false",
            "core.fsmonitor=false",
            "protocol.allow=never",
            "submodule.recurse=false",
            "i18n.logOutputEncoding=UTF-8",
        ] {
            command.extend(["-c".into(), value.into()]);
        }
        command.extend_from_slice(args);
        let remaining = self.deadline.saturating_duration_since(std::time::Instant::now());
        ensure!(!remaining.is_zero(), "{}", stopped_read("timed_out"));
        #[cfg(test)]
        self.processes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result =
            self.runner.run(&self.root, &command, &self.cancel, remaining, 32 * 1024 * 1024)?;
        if !result.stopped {
            return Err(StopUnconfirmed.into());
        }
        if result.reason == "cancelled" {
            return Err(GitCancelled.into());
        }
        ensure!(result.reason == "exited", "{}", stopped_read(result.reason));
        Ok(result)
    }
    // Read only by the Git tests, which build with the contract driver (see `mod tests`).
    #[cfg(all(test, feature = "p08-contract-driver"))]
    pub(super) fn processes(&self) -> usize {
        self.processes.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn command(&self, args: &[&str]) -> Result<Vec<u8>> {
        self.command_owned(args.iter().map(|arg| arg.to_string()).collect())
    }
    fn command_owned(&self, args: Vec<String>) -> Result<Vec<u8>> {
        let result = self.raw(&args)?;
        if result.exit_code != Some(0) {
            anyhow::bail!("{}", config::failure(&result.stderr));
        }
        Ok(result.stdout)
    }
    fn oid(&self, revision: &str) -> Result<String> {
        ensure!(
            !revision.is_empty()
                && revision.len() <= 1024
                && !revision.starts_with('-')
                && !revision.chars().any(char::is_control),
            "git.invalid_revision: 「{}」不是可用的提交名称：不能为空、不能以 - 开头、不能含控制字符，最长 1024 字节。请写提交号、分支名或标签名。",
            config::shown(revision)
        );
        // rev-parse --end-of-options is available since Git 2.20; validated OIDs are used afterwards.
        let found = self
            .command(&["rev-parse", "--verify", "--end-of-options", &format!("{revision}^{{commit}}")])
            .map_err(|error| {
                if error.to_string().starts_with("git.revision_missing") {
                    anyhow::anyhow!(
                        "git.revision_missing: 这个仓库里没有提交「{}」。请核对提交号、分支名或标签名（git.log 可以列出提交）。",
                        config::shown(revision)
                    )
                } else {
                    error
                }
            })?;
        let oid = String::from_utf8(found).unwrap_or_default();
        let oid = oid.trim();
        ensure!(
            (oid.len() == 40 || oid.len() == 64)
                && oid.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "git.invalid_object_id: Git 没有把「{}」解析成完整的提交号，无法继续。请改用完整的提交号再试。",
            config::shown(revision)
        );
        Ok(oid.into())
    }
}

/// A Git read stopped before its end, by the stop reason the runner gave (`timed_out`,
/// `output_limit` or `process_error`).
fn stopped_read(reason: &str) -> &'static str {
    match reason {
        "timed_out" => {
            "git.timed_out: Git 读取用完了 30 秒的时限，没有得到结果；仓库可能很大或正被其他程序占用。可以缩小范围（例如用 paths 只读部分文件）后再试。"
        }
        "output_limit" => {
            "git.output_limit: Git 的输出超过 32 MiB 上限，读取被停止，没有得到结果。请缩小范围（例如用 paths 只读部分文件）后再试。"
        }
        _ => {
            "git.process_error: Git 进程出错，没有正常结束，这次读取没有得到结果。请再试一次；仍然失败时请在设置中重新检测 Git。"
        }
    }
}

/// A repository folder that could not be opened or checked again. Aura's own `git.*` refusals
/// pass unchanged; an operating-system error gets a sentence around it.
fn directory_error(error: std::io::Error) -> anyhow::Error {
    let text = error.to_string();
    if text.starts_with("git.") {
        return error.into();
    }
    anyhow::anyhow!(
        "git.repository_unreadable: 无法打开或核对仓库的目录，这次 Git 操作没有执行。请确认工作区文件夹仍然存在并且可以访问后再试。{}{}",
        config::DETAIL_LABEL,
        config::detail(&text)
    )
}

fn paths(args: &mut Vec<String>, paths: &[String]) -> Result<()> {
    ensure!(
        paths.len() <= 200,
        "git.too_many_paths: paths 最多 200 项，这次有 {} 项。请分几次调用，或改用它们所在的文件夹。",
        paths.len()
    );
    args.push("--".into());
    for path in paths {
        let parsed = aura_domain_common::WorkspacePath::parse(path).map_err(|error| {
            anyhow::anyhow!(
                "git.path_invalid: paths 里的「{}」不是工作区里的相对路径（{error}）。请写相对于工作区的路径，例如 src/main.rs。",
                config::shown(path)
            )
        })?;
        args.push(if parsed.is_root() { ".".into() } else { parsed.to_string() });
    }
    Ok(())
}
#[cfg(all(test, feature = "p08-contract-driver"))]
mod tests;
