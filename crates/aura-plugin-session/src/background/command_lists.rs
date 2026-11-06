//! The chat's commands that list or change where it is: the conversations it can go to and a new
//! one, the conversation's state, its model and its permission mode. Lists are numbered from 1 and
//! name the one in use; the number picks one. A wider permission mode is taken only under the
//! rules a chat's decisions on approvals meet; while the plugin's chats may not decide them, it
//! stays on the computer.
use super::bridge::failed;
use super::commands::{Asked, Done, said};
use super::messages::{chat_title, message_parts};
use super::sessions::{ServiceSessions, checked_title, create, open_now, projection};
use crate::channels::{Refused, check_widening, in_scope};
use crate::checks::command_of;
use crate::service::PluginSessions;
use aura_agent_application::agent_application::AgentApplication;
use aura_agent_domain::{
    AgentSessionRequest, AgentSessionWaitingFor, PluginSessionActivity, PluginSessionRequest,
};
use aura_domain_common::{CorrelationId, SessionId};
use aura_persistence::PersistenceError;
use aura_plugin_host::ServiceError;
use aura_projection_contract::ProjectionChange;
use aura_security_contract::PermissionMode;
use aura_security_service::p04::permission_defaults::default_permission_mode;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

/// The most conversations `/sessions` lists, the newest first.
pub const CHAT_SESSIONS_LISTED: usize = 20;
/// What a chat is told when it asks for a wider permission mode while it may not decide approvals.
const WIDER_ON_COMPUTER: &str = "放宽权限要在电脑上的 Aura 里切。";

/// The modes, from the one that allows least.
const MODES: [(PermissionMode, &str); 3] = [
    (PermissionMode::ReadOnly, "只读"),
    (PermissionMode::WorkspaceReadWrite, "工作区读写"),
    (PermissionMode::FullAccess, "完全访问"),
];

fn reach(mode: PermissionMode) -> usize {
    MODES.iter().position(|(listed, _)| *listed == mode).unwrap_or(0)
}

/// What `/command 编号` names, numbered from 1.
fn numbered<'a, T>(items: &'a [T], typed: &str) -> Option<&'a T> {
    let number: usize = typed.parse().ok()?;
    items.get(number.checked_sub(1)?)
}

fn interrupted(_: tokio::task::JoinError) -> ServiceError {
    ServiceError::new("aura.failed", "做到一半被打断了，再试一次")
}

/// A conversation as a chat lists it.
struct Listed {
    session: SessionId,
    title: String,
    workspace: String,
}

/// The scope's conversations, the newest first.
fn listed(agent: &AgentApplication, asked: &Asked<'_>) -> Result<Vec<Listed>, ServiceError> {
    let projection = projection(agent)?;
    let (_, sessions) = open_now(&projection);
    let scope = &asked.granted.remote.scope;
    let workspace_name = |id| {
        let found = projection.workspaces.iter().find(|workspace| workspace.workspace_id == id);
        found.map_or_else(String::new, |workspace| workspace.display_name.clone())
    };
    Ok(sessions
        .into_iter()
        .rev()
        .filter(|session| in_scope(scope, session.session_id, session.workspace_id))
        .take(CHAT_SESSIONS_LISTED)
        .map(|session| Listed {
            session: session.session_id,
            title: session.title.clone(),
            workspace: workspace_name(session.workspace_id),
        })
        .collect())
}

/// The conversation's permission mode and its revision, set to the default mode the first time
/// it is read, as the composer's selector reads it.
async fn permission_state(
    agent: &Arc<AgentApplication>,
    session: SessionId,
) -> Result<(PermissionMode, u64), ServiceError> {
    let agent = Arc::clone(agent);
    let state = tokio::task::spawn_blocking(move || -> anyhow::Result<_> {
        let persistence = &agent.execution.persistence;
        let state = match persistence.session_permission_state(session) {
            Err(PersistenceError::PermissionStateNotFound) => {
                let workspace = persistence
                    .workspace_session_projection()?
                    .sessions
                    .into_iter()
                    .find(|found| found.session_id == session)
                    .map(|found| found.workspace_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("plugin_session.session_missing: 这个会话已经不在了")
                    })?;
                let mode = default_permission_mode(&agent.settings)
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                let correlation = CorrelationId::new();
                persistence.ensure_session_permission_state(
                    session,
                    workspace,
                    mode,
                    correlation,
                )?
            }
            other => other?,
        };
        Ok((state.mode, state.revision))
    });
    state.await.map_err(interrupted)?.map_err(|error| failed(&error))
}

impl ServiceSessions {
    /// `/sessions`.
    pub(super) fn sessions_listed(
        &self,
        agent: &AgentApplication,
        asked: &Asked<'_>,
    ) -> Result<Done, ServiceError> {
        let listed = listed(agent, asked)?;
        if listed.is_empty() {
            return Ok(said("插件的会话范围里还没有会话。", asked.current));
        }
        let mut text = String::from("会话（/switch 编号 切换）：");
        for (at, found) in listed.iter().enumerate() {
            let mark = if Some(found.session) == asked.current { "（当前）" } else { "" };
            let _ = write!(text, "\n{}. {} · {}{mark}", at + 1, found.title, found.workspace);
        }
        Ok(said(text, asked.current))
    }

    /// `/switch 编号`: the chat goes on in that conversation of `/sessions`.
    pub(super) async fn switch(
        &self,
        agent: &AgentApplication,
        asked: &Asked<'_>,
    ) -> Result<Done, ServiceError> {
        let listed = listed(agent, asked)?;
        let Some(found) = numbered(&listed, asked.arguments) else {
            let text = if asked.arguments.is_empty() {
                "/switch 后面要写上会话的编号，用 /sessions 看有哪些。".to_owned()
            } else {
                format!("没有编号 {} 的会话，用 /sessions 看有哪些。", asked.arguments)
            };
            return Ok(said(text, asked.current));
        };
        if Some(found.session) == asked.current {
            return Ok(said(format!("这个聊天已经在会话 {} 里。", found.title), asked.current));
        }
        self.remember(&asked.granted.plugin, &asked.message.chat, found.session).await;
        Ok(said(format!("已切换到会话 {}。", found.title), Some(found.session)))
    }

    /// `/new [标题]`: a conversation in the scope's first workspace, where the chat goes on.
    pub(super) async fn new_session(
        &self,
        agent: &Arc<AgentApplication>,
        asked: &Asked<'_>,
    ) -> Result<Done, ServiceError> {
        let title = if asked.arguments.is_empty() {
            chat_title(asked.granted, asked.message)
        } else {
            asked.arguments.to_owned()
        };
        if let Err(error) = checked_title(&title) {
            return Ok(said(format!("{}。", error.message), asked.current));
        }
        let projection = projection(agent)?;
        let (open, _) = open_now(&projection);
        let scope = &asked.granted.remote.scope;
        let Some(workspace) = scope.workspaces.iter().copied().find(|id| open.contains(id)) else {
            let text =
                "插件的会话范围里没有工作区，不能新建会话；在 Aura 的 设置 → 插件 里可以改范围。";
            return Ok(said(text, asked.current));
        };
        let agent = self.agent(true)?;
        // The same message again finds the conversation it made.
        let command = command_of(
            "aura plugin chat new session v1",
            &message_parts(&asked.granted.plugin, asked.message),
        );
        let created = create(&agent, workspace, title, command).await?;
        self.remember(&asked.granted.plugin, &asked.message.chat, created.session_id).await;
        let text = format!("已新建会话 {}，这个聊天已换过去。", created.title);
        Ok(said(text, Some(created.session_id)))
    }

    /// `/status`: what the conversation is and is doing.
    pub(super) async fn status(
        &self,
        agent: &Arc<AgentApplication>,
        asked: &Asked<'_>,
        session: SessionId,
    ) -> Result<Done, ServiceError> {
        let request =
            PluginSessionRequest { plugin: asked.granted.plugin.clone(), session_id: session };
        let panel = PluginSessions::new(Arc::clone(agent), Arc::clone(&self.plugins));
        let state = panel.state(request).await.map_err(|error| failed(&error))?;
        let projection = projection(agent)?;
        let workspace = projection
            .sessions
            .iter()
            .find(|found| found.session_id == session)
            .and_then(|found| {
                let id = found.workspace_id;
                projection.workspaces.iter().find(|workspace| workspace.workspace_id == id)
            })
            .map_or("", |workspace| workspace.display_name.as_str());
        let (mode, _) = permission_state(agent, session).await?;
        let inbox = agent.inbox(AgentSessionRequest { session_id: session }).await;
        let queued = inbox.map_err(|error| failed(&error))?.messages.len();
        let activity = match (state.activity, state.waiting_for) {
            (PluginSessionActivity::Idle, _) => "空闲",
            (_, Some(AgentSessionWaitingFor::Approval)) => "等你批准",
            (_, Some(AgentSessionWaitingFor::Question)) => "等你回答",
            (_, Some(AgentSessionWaitingFor::Context)) => "等你决定上下文怎么处理",
            _ => "运行中",
        };
        let model = state.model_name.as_deref().unwrap_or("还没有选");
        let mut text = format!(
            "会话：{}\n工作区：{workspace}\n模型：{model}\n权限：{}\n状态：{activity}",
            state.title,
            MODES[reach(mode)].1
        );
        if queued > 0 {
            let _ = write!(text, "\n排队的消息：{queued} 条");
        }
        Ok(said(text, Some(session)))
    }

    /// `/model [编号]`: the models the conversation can use, or one of them for it.
    pub(super) async fn model(
        &self,
        agent: &Arc<AgentApplication>,
        session: SessionId,
        typed: &str,
    ) -> Result<Done, ServiceError> {
        let reading = Arc::clone(agent);
        let selector = tokio::task::spawn_blocking(move || reading.p05.session_selector(session))
            .await
            .map_err(interrupted)?
            .map_err(|error| failed(&error))?;
        let ProjectionChange::SessionModelSelector { payload, .. } = selector else {
            return Err(ServiceError::new("aura.failed", "读不出这个会话的模型"));
        };
        let mut models: Vec<_> =
            payload.models.iter().filter(|model| model.execution_readiness.is_executable).collect();
        models.sort_by_key(|model| model.order);
        let current = payload.selected_model_profile_id;
        if typed.is_empty() {
            if models.is_empty() {
                return Ok(said("没有能用的模型，要在电脑上的 Aura 里配置。", Some(session)));
            }
            let mut text = String::from("能用的模型（/model 编号 切换）：");
            for (at, model) in models.iter().enumerate() {
                let mark =
                    if Some(model.model_profile_id) == current { "（当前）" } else { "" };
                let _ = write!(text, "\n{}. {}{mark}", at + 1, model.display_name);
            }
            return Ok(said(text, Some(session)));
        }
        let Some(chosen) = numbered(&models, typed) else {
            let text = format!("没有编号 {typed} 的模型，用 /model 看能用的。");
            return Ok(said(text, Some(session)));
        };
        let (id, name) = (chosen.model_profile_id, chosen.display_name.clone());
        if Some(id) == current {
            return Ok(said(format!("已经是模型 {name}。"), Some(session)));
        }
        let binding = Arc::clone(agent);
        tokio::task::spawn_blocking(move || {
            binding.p05.bind_model(session, id, CorrelationId::new())
        })
        .await
        .map_err(interrupted)?
        .map_err(|error| ServiceError::new("aura.failed", error.message))?;
        Ok(said(format!("已切换到模型 {name}。"), Some(session)))
    }

    /// `/permission [编号]`: the modes, or another one for the conversation: a narrower one at once,
    /// a wider one under the rules of `channels::approvals`.
    pub(super) async fn permission(
        &self,
        agent: &Arc<AgentApplication>,
        asked: &Asked<'_>,
        session: SessionId,
    ) -> Result<Done, ServiceError> {
        let (typed, plugin, chat) =
            (asked.arguments, asked.granted.plugin.as_str(), &asked.message.chat);
        let (mode, revision) = permission_state(agent, session).await?;
        if typed.is_empty() {
            let mut text = String::from("权限模式（/permission 编号 切换）：");
            for (at, (listed, name)) in MODES.iter().enumerate() {
                let mark = if *listed == mode { "（当前）" } else { "" };
                let _ = write!(text, "\n{}. {name}{mark}", at + 1);
            }
            if let Ok(mut approvals) = self.approvals.lock() {
                approvals.listed(plugin, chat, Instant::now());
            }
            return Ok(said(text, Some(session)));
        }
        let Some((wanted, name)) = numbered(&MODES, typed).copied() else {
            let text = format!("没有编号 {typed} 的权限模式，用 /permission 看有哪些。");
            return Ok(said(text, Some(session)));
        };
        if wanted == mode {
            return Ok(said(format!("权限已经是 {name}。"), Some(session)));
        }
        if reach(wanted) > reach(mode) {
            let listed = self.approvals.lock().ok().and_then(|found| found.listed_at(plugin, chat));
            let on = asked.granted.remote.remote_approval;
            match check_widening(on, chat, listed, Instant::now()) {
                Ok(()) => {}
                Err(Refused::Off) => return Ok(said(WIDER_ON_COMPUTER, Some(session))),
                Err(refused) => return Ok(said(refused.text(), Some(session))),
            }
        }
        let correlation = CorrelationId::new();
        let persistence = &agent.execution.persistence;
        persistence
            .change_session_permission_mode(session, revision, wanted, correlation)
            .await
            .map_err(|error| ServiceError::new("aura.failed", format!("没有切成：{error}")))?;
        // What the new mode no longer allows stops, as when the user switches.
        if let Err(error) = agent.mode_changed(session, mode).await {
            let text = format!("权限已切换到 {name}，正在运行的操作还没有确认停下：{error}");
            return Ok(said(text, Some(session)));
        }
        Ok(said(format!("权限已切换到 {name}。"), Some(session)))
    }
}
