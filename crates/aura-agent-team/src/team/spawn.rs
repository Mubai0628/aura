//! `agents.spawn`: a new sub-agent in a hidden conversation of its own, working on its task sheet.
use super::*;
use crate::{Allowances, Grant, MemberDetails, ModelOffer, Worktree, policy, worktree as rows};
use aura_domain_common::ModelProfileId;
use aura_security_contract::PermissionMode;

const NAME_MAX_CHARS: usize = 40;

impl<H: TeamHost> Team<'_, H> {
    pub(super) async fn spawn(&self, call: &TeamCall) -> anyhow::Result<AgentsSpawnResult> {
        let input: AgentsSpawnInput = parse(&call.arguments)?;
        let spawn_call = call.tool_call_id.to_string();
        let parent_id = call.session_id;
        // The same call made again answers with the sub-agent it started.
        let key = spawn_call.clone();
        if let Some((member, worktree)) = self
            .host
            .read("agents.spawn.repeat", move |db| {
                let Some(member) = store::member_by_spawn(db, &key)? else { return Ok(None) };
                let worktree = rows::worktree_of(db, member.child_session_id)?;
                Ok(Some((member, worktree)))
            })
            .await?
        {
            return Ok(spawned(&member, worktree.as_ref()));
        }
        self.refuse_subagent(parent_id).await?;
        let name = input.name.trim().to_owned();
        if name.is_empty()
            || name.chars().count() > NAME_MAX_CHARS
            || name.chars().any(char::is_control)
        {
            return Err(Refusal::new(
                "agents.name_invalid",
                format!("name 写一个不超过 {NAME_MAX_CHARS} 字的简短任务名。"),
            )
            .into());
        }
        input.task.check()?;
        self.take_slot(parent_id).await?;
        let parent = self.host.parent(parent_id).await?;
        if input.worktree && !parent.git {
            return Err(Refusal::new(
                "agents.not_git",
                "这个工作区不是 Git 仓库，建不了工作副本；子 Agent 只能只读地干活（不写 worktree）。",
            )
            .into());
        }
        let model = choose_model(&self.host.models()?, input.model.as_deref(), parent.model)?;
        let tools = policy::granted_tools(input.tools.as_deref(), &call.own_tools)?;
        let (_, permissions, _) =
            self.host.read("agents.spawn.settings", crate::limits::settings).await?;
        let mode = permissions.starting_mode(input.worktree);
        let delegated_reads =
            self.host.delegated_reads(call.session_id, call.turn_task_id, input.read_paths).await?;
        let placed = match input.worktree {
            true => Some(self.place_worktree(parent.workspace_id, call.tool_call_id).await?),
            false => None,
        };
        let opened = self
            .host
            .open(
                &parent,
                placed.as_ref().map(|placed| placed.worktree.workspace_id),
                &name,
                model.id,
                mode,
                command_for(call.tool_call_id, "open"),
            )
            .await;
        let child = match opened {
            Ok(child) => child,
            Err(error) => {
                if let Some(placed) = &placed {
                    self.take_back(&placed.worktree, call.tool_call_id).await;
                }
                return Err(super::failure::not_opened(error));
            }
        };
        let carried = placed.as_ref().map_or(0, |placed| placed.carried);
        let worktree = placed.map(|placed| Worktree {
            session_id: child,
            parent_session_id: Some(parent_id),
            ..placed.worktree
        });
        let now = UtcTimestamp::now();
        let mut member = Member {
            child_session_id: child,
            parent_session_id: parent_id,
            handle: String::new(),
            spawn_call,
            state: MemberState::Running,
            revision: 1,
            details: MemberDetails {
                name,
                model_profile_id: model.id,
                model_name: model.name.clone(),
                sheet: input.task,
                grant: Grant { tools, shared: worktree.is_none() },
                delegated_reads,
                mode: Some(mode),
                // What the settings let a new sub-agent's commands do; deleting without asking is
                // only ever in a worktree, whose files its branch keeps.
                allowed: Allowances {
                    deletions: permissions.allow_deletions && worktree.is_some(),
                    outside_writes: permissions.allow_outside_writes,
                    downloads: permissions.allow_downloads,
                },
                reports_seen: 0,
                last_turn: None,
                reviews: Vec::new(),
            },
            created_at: now,
            updated_at: now,
        };
        let row = worktree.clone();
        let inserted = self
            .host
            .write("agents.spawn.member", move |tx| {
                member.handle = store::next_handle(tx, parent_id)?;
                store::insert_member(tx, &member)?;
                if let Some(row) = &row {
                    rows::insert_worktree(tx, row)?;
                }
                Ok(member)
            })
            .await;
        let member = match inserted {
            Ok(member) => member,
            Err(error) => {
                // Nothing was started in it: the conversation and its worktree go again.
                let _ = self.host.discard(child).await;
                if let Some(worktree) = &worktree {
                    self.take_back(worktree, call.tool_call_id).await;
                }
                return Err(error);
            }
        };
        let mut sheet = member.details.sheet.render(&member.details.name);
        let narrowed = member.details.grant.tools.as_deref();
        let has = |name: &str| narrowed.is_none_or(|tools| tools.iter().any(|tool| tool == name));
        if has("shell.cmd") || has("shell.powershell") {
            sheet.push_str(COMMANDS_NOTE);
        }
        match &worktree {
            Some(worktree) => sheet.push_str(&worktree_note(worktree, carried, &has)),
            None => sheet.push_str(SHARED_NOTE),
        }
        sheet.push_str(&mode_note(mode));
        if let Some(reads) = &member.details.delegated_reads {
            sheet.push_str("\nAuthorized read-only resources for this task. Use the absolute paths with fs.read, fs.list, fs.find or fs.search:");
            for scope in &reads.scopes {
                sheet.push_str("\n- ");
                sheet.push_str(&scope.path);
            }
        }
        if let Err(error) =
            self.host.deliver(child, sheet, command_for(call.tool_call_id, "start")).await
        {
            // It never started: it is stopped, and the reason stays with it.
            let stopped =
                self.change(&member, |next| next.state = MemberState::Stopped).await.is_ok();
            let said = super::failure::start_failed(&member, worktree.is_some(), stopped, &error);
            return Err(anyhow::anyhow!(said));
        }
        self.news.notify_waiters();
        Ok(spawned(&member, worktree.as_ref()))
    }
}

fn spawned(member: &Member, worktree: Option<&Worktree>) -> AgentsSpawnResult {
    AgentsSpawnResult {
        agent: member.handle.clone(),
        name: member.details.name.clone(),
        model: member.details.model_name.clone(),
        tools: member.details.grant.tools.clone(),
        mode: member.details.mode.unwrap_or(if member.details.grant.shared {
            PermissionMode::ReadOnly
        } else {
            PermissionMode::WorkspaceReadWrite
        }),
        state: member.state,
        branch: worktree.map(|worktree| worktree.details.branch.clone()),
    }
}

/// What the task sheet says about where a sub-agent's commands run.
const COMMANDS_NOTE: &str = "\n## 命令\n你的命令在你自己的进程组里跑，CPU 和内存有上限；TEMP 和 TMP 是你自己的临时文件夹。\
    起要用端口的服务时，用环境变量 AURA_PORT_RANGE 里的端口（PORT 是其中第一个，除非用户自己设了 PORT），\
    免得和别的子 Agent 撞上。\n";

/// What the task sheet adds for a sub-agent that shares the workspace.
const SHARED_NOTE: &str =
    "\n## 工作区\n你和主 Agent 共用这个工作区：这里的文件就是用户的文件，主 Agent 也在用。\n";

/// What the task sheet says about the permission mode a sub-agent starts in, which the user may
/// change while it works.
fn mode_note(mode: PermissionMode) -> String {
    let (name, means) = match mode {
        PermissionMode::ReadOnly => {
            ("只读", "改文件和跑命令都要先等用户批准，所以只在读文件答不了时才跑命令")
        }
        PermissionMode::WorkspaceReadWrite => (
            "工作区读写",
            "在你的工作区里改文件、用 fs.delete 删文件（进回收站）不用问；命令里删文件、写工作区以外和下载可能要先等批准",
        ),
        PermissionMode::FullAccess => ("完全访问", "改文件和跑命令都不用问"),
    };
    format!(
        "\n## 权限\n你的权限档位是「{name}」：{means}。用户可能中途换档，以工具的实际结果为准。\n"
    )
}

/// What the task sheet adds for a sub-agent working in a worktree, naming the writing tools it
/// has and the workspace's uncommitted files it took along.
fn worktree_note(worktree: &Worktree, carried: usize, has: &impl Fn(&str) -> bool) -> String {
    let start: String = worktree.details.base.chars().take(8).collect();
    let start = if carried == 0 {
        format!("从提交 {start} 开始")
    } else {
        format!(
            "从提交 {start} 开始；这个提交带着建副本时主工作区还没提交的 {carried} 个文件改动，主工作区里看得到的文件这里都有"
        )
    };
    let writes: Vec<&str> =
        ["fs.write", "fs.patch", "fs.delete"].into_iter().filter(|tool| has(tool)).collect();
    let mut can = Vec::new();
    if !writes.is_empty() {
        can.push(format!("用 {} 改这个副本里的文件", writes.join("、")));
    }
    if has("git.commit") {
        can.push("用 git.commit 提交到这个分支".to_owned());
    }
    let can = if can.is_empty() {
        "这次给你的工具不改文件".to_owned()
    } else {
        format!("你可以{}", can.join("，"))
    };
    format!(
        "\n## 工作副本\n你在自己的 Git 工作副本里干活：文件夹 {}，分支 {}（{start}）。\
         {can}；改动只在这个副本里，主工作区和别的子 Agent 都不受影响。\
         做完在这一轮的最终回答里写清改了哪些文件、跑了哪些检查和它们的退出码；合回主工作区由主 Agent 决定。\n",
        worktree.details.path, worktree.details.branch
    )
}

/// The model a new sub-agent uses: the one asked for, else the conversation's own, else the first
/// one the user allowed for sub-agents.
pub fn choose_model(
    models: &[ModelOffer],
    asked: Option<&str>,
    own: Option<ModelProfileId>,
) -> Result<ModelOffer, Refusal> {
    let allowed: Vec<&ModelOffer> = models.iter().filter(|model| model.for_subagents).collect();
    let Some(&first) = allowed.first() else {
        return Err(Refusal::new(
            "agents.no_model",
            "没有能给子 Agent 用的模型：请用户在 设置 → 模型 里对模型选“允许子 Agent”。",
        ));
    };
    if let Some(asked) = asked.map(str::trim).filter(|asked| !asked.is_empty()) {
        return allowed
            .iter()
            .find(|model| model.id.to_string() == asked || model.name.eq_ignore_ascii_case(asked))
            .map(|model| (*model).clone())
            .ok_or_else(|| {
                let names: Vec<&str> = allowed.iter().map(|model| model.name.as_str()).collect();
                Refusal::new(
                    "agents.model_not_allowed",
                    format!("{asked} 不能给子 Agent 用。可用的模型：{}。", names.join("、")),
                )
            });
    }
    Ok(own
        .and_then(|own| allowed.iter().find(|model| model.id == own))
        .copied()
        .unwrap_or(first)
        .clone())
}
