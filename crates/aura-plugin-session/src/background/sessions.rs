//! A plugin's background service in the conversations the user put in its scope: which they are,
//! a new one, and reading, sending to, running the plugin's own command in and stopping one as
//! its panel does in its own. A conversation outside the scope is refused before anything is read
//! or done there. Starting one, sending and running a command are new Agent work, refused with
//! the real reason while new work cannot start.
use super::bridge::{NoParams, failed, parsed, with_ready};
use super::messages::InOrder;
use crate::channels::{PendingReplies, RemoteApprovals, SeenCommands, in_scope};
use crate::checks::{checked_key, command_of};
use crate::service::PluginSessions;
use aura_agent_application::agent_application::AgentApplication;
use aura_agent_domain::{
    PluginCommandRunRequest, PluginSessionEventsRequest, PluginSessionRequest,
    PluginSessionSendRequest,
};
use aura_core_base::append_core_log;
use aura_domain_common::{CommandInvocationId, CorrelationId, SessionId, WorkspaceId};
use aura_extension_service::plugins::PluginRuntime;
use aura_persistence::{PersistenceError, SESSION_ARCHIVED};
use aura_plugin_host::{CallFuture, ServiceError};
use aura_plugins::{PluginCapability, PluginChannel, PluginRemote, PluginScope};
use aura_security_service::p04::permission_defaults::default_permission_mode;
use aura_workspace_domain::{
    SessionCreateRequest, SessionSummary, WorkspaceSessionProjection, WorkspaceState,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, OnceLock};

/// The most conversations one `sessions.list` names, the newest first.
pub const LISTED_SESSIONS: usize = 200;
/// The longest title a conversation may have.
const TITLE_MAX_CHARS: usize = 200;

/// The Agent application for a call; `true` when the call starts Agent work.
pub type AgentAccess = Arc<dyn Fn(bool) -> anyhow::Result<Arc<AgentApplication>> + Send + Sync>;

/// What a call may use of the plugin, as it is trusted now.
pub(crate) struct Granted {
    pub(crate) plugin: String,
    pub(crate) remote: PluginRemote,
    pub(crate) channels: Vec<PluginChannel>,
    pub(crate) capabilities: Vec<PluginCapability>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewParams {
    #[serde(default)]
    workspace_id: Option<WorkspaceId>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    dedupe_key: Option<String>,
}

pub(crate) struct ServiceSessions {
    pub(crate) plugins: Arc<PluginRuntime>,
    pub(crate) runtime: tokio::runtime::Handle,
    agent: OnceLock<AgentAccess>,
    pub(crate) replies: Mutex<PendingReplies>,
    pub(crate) in_order: InOrder,
    /// The command messages each plugin's chats sent, so each runs once.
    pub(crate) seen: Mutex<SeenCommands>,
    /// The approval each plugin handed to one of its chats, and those waiting behind it.
    pub(crate) approvals: Mutex<RemoteApprovals>,
}

impl ServiceSessions {
    pub(crate) fn new(plugins: Arc<PluginRuntime>, runtime: tokio::runtime::Handle) -> Arc<Self> {
        Arc::new(Self {
            plugins,
            runtime,
            agent: OnceLock::new(),
            replies: Mutex::default(),
            in_order: InOrder::default(),
            seen: Mutex::default(),
            approvals: Mutex::default(),
        })
    }

    /// The capability a conversation call needs; `None` for a method that is not one.
    pub(crate) fn needs(method: &str) -> Option<PluginCapability> {
        Some(match method {
            "sessions.list" | "session.state" => PluginCapability::SessionRead,
            "session.events" => PluginCapability::SessionEvents,
            "session.send" | "message.in" => PluginCapability::SessionSend,
            "session.new" | "session.stop" => PluginCapability::SessionControl,
            "command.run" => PluginCapability::Commands,
            _ => return None,
        })
    }

    /// Core's way to its Agent application; the calls are refused until there is one.
    pub(crate) fn attach(&self, agent: AgentAccess) {
        let _ = self.agent.set(agent);
    }

    /// The Agent application to read with, when there is one.
    pub(crate) fn reading(&self) -> Option<Arc<AgentApplication>> {
        self.agent.get().and_then(|agent| agent(false).ok())
    }

    pub(crate) fn agent(&self, starts_work: bool) -> Result<Arc<AgentApplication>, ServiceError> {
        let agent = self.agent.get().ok_or_else(|| {
            ServiceError::new("aura.not_ready", "Aura 的会话还没有准备好，稍后再试")
        })?;
        agent(starts_work).map_err(|error| failed(&error))
    }

    /// The service ended: its messages still waiting are taken, and no more are; the approval it
    /// was handed goes out again to the one started after it.
    pub(crate) fn ended(&self, plugin: &str) {
        self.in_order.end(plugin);
        if let Ok(mut approvals) = self.approvals.lock() {
            approvals.service_ended(plugin);
        }
    }

    /// The service no longer runs: nothing more is owed to its chats.
    pub(crate) fn forget(&self, plugin: &str) {
        if let Ok(mut replies) = self.replies.lock() {
            replies.forget(plugin);
        }
        if let Ok(mut seen) = self.seen.lock() {
            seen.forget(plugin);
        }
        if let Ok(mut approvals) = self.approvals.lock() {
            approvals.forget(plugin);
        }
    }

    /// A conversation call, once the plugin may make it: answered by Core's runtime, a message
    /// from a channel behind the plugin's earlier ones.
    pub(crate) fn call(
        self: &Arc<Self>,
        plugin: &str,
        method: &str,
        needed: PluginCapability,
        params: Value,
    ) -> CallFuture {
        let granted = with_ready(&self.plugins, plugin, needed, |ready| {
            Ok(Granted {
                plugin: plugin.to_owned(),
                remote: ready.setting.remote.clone(),
                channels: ready.package.manifest.declared.channels.clone(),
                capabilities: ready.package.manifest.declared.capabilities.clone(),
            })
        });
        let granted = match granted {
            Ok(granted) => granted,
            Err(error) => return Box::pin(std::future::ready(Err(error))),
        };
        if method == "message.in" {
            return self.in_order.take(self, granted, params);
        }
        let (this, method) = (Arc::clone(self), method.to_owned());
        Box::pin(async move { this.run(&method, &granted, params).await })
    }

    async fn run(
        &self,
        method: &str,
        granted: &Granted,
        params: Value,
    ) -> Result<Value, ServiceError> {
        let agent = self.agent(matches!(method, "session.new" | "session.send" | "command.run"))?;
        let (plugin, scope) = (granted.plugin.as_str(), &granted.remote.scope);
        let panel = || PluginSessions::new(Arc::clone(&agent), Arc::clone(&self.plugins));
        match method {
            "sessions.list" => listed(&agent, scope, params),
            "session.new" => opened(&agent, granted, params).await,
            "session.state" => {
                let request: PluginSessionRequest = as_plugin(plugin, params)?;
                reachable(&agent, scope, request.session_id)?;
                answer(panel().state(request).await)
            }
            "session.events" => {
                let request: PluginSessionEventsRequest = as_plugin(plugin, params)?;
                reachable(&agent, scope, request.session_id)?;
                answer(panel().events(request).await)
            }
            "session.send" => {
                let request: PluginSessionSendRequest = as_plugin(plugin, params)?;
                reachable(&agent, scope, request.session_id)?;
                answer(panel().send(CommandInvocationId::new(), request).await)
            }
            "command.run" => {
                let request: PluginCommandRunRequest = as_plugin(plugin, params)?;
                reachable(&agent, scope, request.session_id)?;
                answer(panel().run(CommandInvocationId::new(), request).await)
            }
            _ => {
                let request: PluginSessionRequest = as_plugin(plugin, params)?;
                reachable(&agent, scope, request.session_id)?;
                answer(panel().stop(request).await)
            }
        }
    }
}

/// The request as the panel's call names it, for the plugin the service belongs to.
fn as_plugin<T: DeserializeOwned>(plugin: &str, params: Value) -> Result<T, ServiceError> {
    let Value::Object(mut fields) = params else { return parsed(params) };
    if fields.insert("plugin".to_owned(), Value::from(plugin)).is_some() {
        return Err(ServiceError::new(
            "protocol.bad_params",
            "参数不对：plugin 由 Aura 填，不用传",
        ));
    }
    parsed(Value::Object(fields))
}

fn answer<T: Serialize>(outcome: anyhow::Result<T>) -> Result<Value, ServiceError> {
    let value = outcome.map_err(|error| failed(&error))?;
    serde_json::to_value(value).map_err(|error| ServiceError::new("aura.failed", error.to_string()))
}

pub(crate) fn out_of_scope() -> ServiceError {
    ServiceError::new(
        "plugin_session.out_of_scope",
        "这个会话不在插件的会话范围里，在 设置 → 插件 里可以改范围",
    )
}

pub(crate) fn projection(
    agent: &AgentApplication,
) -> Result<WorkspaceSessionProjection, ServiceError> {
    agent
        .execution
        .persistence
        .workspace_session_projection()
        .map_err(|error| ServiceError::new("aura.failed", format!("读不出会话列表：{error}")))
}

/// The workspaces new conversations may go in, and the conversations of those that are not
/// archived, in the order they were made.
pub(crate) fn open_now(
    projection: &WorkspaceSessionProjection,
) -> (Vec<WorkspaceId>, Vec<&SessionSummary>) {
    let workspaces: Vec<WorkspaceId> = projection
        .workspaces
        .iter()
        .filter(|workspace| workspace.state != WorkspaceState::Archived)
        .map(|workspace| workspace.workspace_id)
        .collect();
    let sessions = projection
        .sessions
        .iter()
        .filter(|session| {
            session.state != SESSION_ARCHIVED && workspaces.contains(&session.workspace_id)
        })
        .collect();
    (workspaces, sessions)
}

/// The conversation exists and is in the scope. One the scope names that is gone is said to be
/// gone; any other is out of the scope, whether it exists or not.
fn reachable(
    agent: &AgentApplication,
    scope: &PluginScope,
    session: SessionId,
) -> Result<(), ServiceError> {
    let projection = projection(agent)?;
    match projection.sessions.iter().find(|found| found.session_id == session) {
        Some(found) if in_scope(scope, found.session_id, found.workspace_id) => Ok(()),
        None if scope.sessions.contains(&session) => {
            Err(ServiceError::new("plugin_session.session_missing", "这个会话已经不在了"))
        }
        _ => Err(out_of_scope()),
    }
}

/// `sessions.list`: the scope's workspaces and the conversations in it, the newest first.
fn listed(
    agent: &AgentApplication,
    scope: &PluginScope,
    params: Value,
) -> Result<Value, ServiceError> {
    let NoParams {} = parsed(params)?;
    let projection = projection(agent)?;
    let (open, sessions) = open_now(&projection);
    let workspaces: Vec<Value> = projection
        .workspaces
        .iter()
        .filter(|workspace| {
            scope.workspaces.contains(&workspace.workspace_id) && open.contains(&workspace.workspace_id)
        })
        .map(|workspace| {
            json!({"workspace_id": workspace.workspace_id, "name": workspace.display_name})
        })
        .collect();
    let mut sessions: Vec<&SessionSummary> = sessions
        .into_iter()
        .rev()
        .filter(|session| in_scope(scope, session.session_id, session.workspace_id))
        .collect();
    let more = sessions.len() > LISTED_SESSIONS;
    sessions.truncate(LISTED_SESSIONS);
    let sessions: Vec<Value> = sessions
        .into_iter()
        .map(|session| {
            json!({
                "session_id": session.session_id,
                "workspace_id": session.workspace_id,
                "title": session.title,
            })
        })
        .collect();
    Ok(json!({"workspaces": workspaces, "sessions": sessions, "more": more}))
}

/// `session.new`: a conversation in a workspace of the scope, the first one when none is named.
/// With a key, the same key again is the same conversation.
async fn opened(
    agent: &Arc<AgentApplication>,
    granted: &Granted,
    params: Value,
) -> Result<Value, ServiceError> {
    let params: NewParams = parsed(params)?;
    let scope = &granted.remote.scope;
    let projection = projection(agent)?;
    let (open, _) = open_now(&projection);
    let workspace = match params.workspace_id {
        Some(asked) if !scope.workspaces.contains(&asked) => return Err(out_of_scope()),
        Some(asked) if !open.contains(&asked) => {
            return Err(ServiceError::new(
                "plugin_session.workspace_missing",
                "这个工作区已经不在了",
            ));
        }
        Some(asked) => asked,
        None => {
            scope.workspaces.iter().copied().find(|workspace| open.contains(workspace)).ok_or_else(
                || {
                    ServiceError::new(
                        "plugin_session.out_of_scope",
                        "插件的会话范围里没有工作区，不能新建会话；在 设置 → 插件 里可以改范围",
                    )
                },
            )?
        }
    };
    let title = params.title.unwrap_or_else(|| granted.plugin.clone());
    checked_title(&title)?;
    let command = match &params.dedupe_key {
        Some(key) => {
            let key = checked_key(key).map_err(|error| failed(&error))?;
            command_of("aura plugin service session v1", &[&granted.plugin, key])
        }
        None => CommandInvocationId::new(),
    };
    let created = create(agent, workspace, title, command).await?;
    Ok(json!({
        "session_id": created.session_id,
        "workspace_id": created.workspace_id,
        "title": created.title,
    }))
}

/// A conversation's title is one line that is not blank, at most [`TITLE_MAX_CHARS`] characters.
pub(crate) fn checked_title(title: &str) -> Result<(), ServiceError> {
    if title.trim().is_empty()
        || title.chars().count() > TITLE_MAX_CHARS
        || title.chars().any(char::is_control)
    {
        return Err(ServiceError::new(
            "plugin_session.title_invalid",
            format!("会话标题要是一行不空的文字，最多 {TITLE_MAX_CHARS} 个字符"),
        ));
    }
    Ok(())
}

/// A new conversation, in the permission mode and with the model the user's own new conversations
/// start with. One whose default model could not be bound stays unbound, as Core's log says; its
/// first message then waits for the user's choice in Aura.
pub(crate) async fn create(
    agent: &Arc<AgentApplication>,
    workspace: WorkspaceId,
    title: String,
    command: CommandInvocationId,
) -> Result<SessionSummary, ServiceError> {
    let mode = default_permission_mode(&agent.settings).map_err(|error| {
        ServiceError::new("aura.failed", format!("读不出新会话的权限模式：{error}"))
    })?;
    let agent = Arc::clone(agent);
    let request = SessionCreateRequest { workspace_id: workspace, title };
    let created = tokio::task::spawn_blocking(move || {
        let correlation = CorrelationId::new();
        let created =
            agent.execution.persistence.create_session(command, request, mode, correlation);
        if let Ok(session) = &created
            && let Err(error) = agent.p05.bind_default_model(session.session_id, correlation)
        {
            append_core_log(
                "plugins.session_model_unbound",
                json!({"session_id": session.session_id, "code": error.code.as_str()}),
            );
        }
        created
    })
    .await
    .map_err(|_| ServiceError::new("aura.failed", "新建会话时被打断了，再试一次"))?;
    created.map_err(|error| match error {
        PersistenceError::InvalidRequest(code) if code == "session.idempotency_conflict" => {
            ServiceError::new(
                "plugin_session.dedupe_key_reused",
                "这个去重键已经用在另一个新会话上，换一个再试",
            )
        }
        other => {
            append_core_log(
                "plugins.session_create_failed",
                json!({"workspace_id": workspace, "reason": other.to_string()}),
            );
            ServiceError::new("aura.failed", "没有建成会话，稍后再试")
        }
    })
}
