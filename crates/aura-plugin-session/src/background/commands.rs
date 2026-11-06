//! What a chat's command does. Aura's own commands act as the composer's and the conversation's
//! own buttons do, and are answered in the chat; one that starts a turn owes the chat that turn's
//! reply, its message marked as the chat's; a prompt command goes to the conversation as its
//! template says; a decision on an approval goes as `approvals` says. A command the plugin did
//! not declare what it needs for, or one that only works on the computer, is answered with why.
//! A command message runs once.
use super::bridge::failed;
use super::messages::{NO_SCOPE_REPLY, message_parts, sender_of};
use super::sessions::{Granted, ServiceSessions, projection};
use crate::channels::{
    Choice, Incoming, Remote, RemoteCommand, Typed, expanded, help_text, remote_command,
};
use crate::checks::command_of;
use crate::service::PluginSessions;
use aura_agent_application::agent_application::AgentApplication;
use aura_agent_domain::{
    AgentCompactionStartRequest, AgentInitStartRequest, AgentSessionRequest, AgentStartRequest,
    AgentTaskMode, CommandEntry, PluginSessionRequest,
};
use aura_context_domain::AgentCompactionRequest;
use aura_core_broker::agent_store::StoreStep;
use aura_domain_common::{CommandInvocationId, EventId, ModelProfileId, SessionId};
use aura_extension_service::commands::command_catalog;
use aura_persistence_v2::agent::{plugin_session, session, turns};
use aura_plugin_host::ServiceError;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Instant;

/// What a chat is told for a command about its conversation before it has one.
const NO_SESSION: &str = "这个聊天还没有会话，发一条消息就会新建一个。";
const NO_MODEL: &str = "这个会话还没有选模型，用 /model 选一个。";

/// One command as the chat asked it.
pub(super) struct Asked<'a> {
    pub(super) granted: &'a Granted,
    pub(super) message: &'a Incoming,
    /// The text after the command.
    pub(super) arguments: &'a str,
    /// Where the chat's next message goes now.
    pub(super) current: Option<SessionId>,
}

/// What a command did: the reply the chat is owed, and the chat's conversation after it.
pub(super) struct Done {
    reply: Owed,
    session: Option<SessionId>,
}

enum Owed {
    Now(String),
    /// The reply of the turn that began with `root`, read in the journal from `cursor` on.
    Turn {
        session: SessionId,
        root: EventId,
        cursor: u64,
    },
}

pub(super) fn said(text: impl Into<String>, session: Option<SessionId>) -> Done {
    Done { reply: Owed::Now(text.into()), session }
}

/// Why a command that starts a turn cannot start one now.
enum Busy {
    Running,
    Queued,
}

impl Busy {
    fn reason(&self, name: &str) -> String {
        match self {
            Self::Running => format!("现在有任务在运行，等它结束再用 /{name}。"),
            Self::Queued => format!("还有排队的消息，等它们处理完再用 /{name}。"),
        }
    }
}

impl ServiceSessions {
    /// Runs the command the message starts with; none when it names no command, and the message
    /// is plain text.
    pub(super) async fn command_in(
        &self,
        granted: &Granted,
        message: &Incoming,
        typed: Typed<'_>,
    ) -> Result<Option<Value>, ServiceError> {
        let agent = self.agent(false)?;
        let current = match self.choice(&agent, granted, message).await? {
            Choice::Use(session) => Some(session),
            Choice::Create(_) | Choice::NoScope => None,
        };
        let catalog = self.catalog_for(&agent, current).await;
        let declared = &granted.capabilities;
        let Some(command) = remote_command(typed.name, &catalog, declared) else {
            return Ok(None);
        };
        let (plugin, name) = (granted.plugin.as_str(), typed.name);
        let key = message_parts(plugin, message)[1..].join("\u{1f}");
        let first =
            self.seen.lock().map_or(true, |mut seen| seen.first(plugin, &key, Instant::now()));
        if !first {
            return Ok(Some(json!({"accepted": true, "command": name, "repeated": true})));
        }
        let done = match command {
            RemoteCommand::Prompt { template } => {
                let text = expanded(&template, typed.arguments);
                let mut answer = self.deliver_message(granted, message, &text).await?;
                answer["command"] = json!(name);
                return Ok(Some(answer));
            }
            RemoteCommand::Desktop => said(format!("/{name} 要在电脑上的 Aura 里用。"), current),
            RemoteCommand::Undeclared(needs) => {
                let needs = needs.name();
                said(format!("这个插件没有声明 {needs}，不能用 /{name}。"), current)
            }
            RemoteCommand::Builtin(remote) => {
                let asked = Asked { granted, message, arguments: typed.arguments, current };
                self.builtin(&agent, &asked, remote, &catalog)
                    .await
                    .unwrap_or_else(|error| said(format!("没有做成：{}", error.message), current))
            }
        };
        self.owe(plugin, message, done.reply);
        let mut answer = json!({"accepted": true, "command": name, "repeated": false});
        if let Some(session) = done.session {
            answer["session_id"] = json!(session);
        }
        Ok(Some(answer))
    }

    async fn builtin(
        &self,
        agent: &Arc<AgentApplication>,
        asked: &Asked<'_>,
        remote: Remote,
        catalog: &[CommandEntry],
    ) -> Result<Done, ServiceError> {
        let current = asked.current;
        let granted = asked.granted;
        match (remote, current) {
            (Remote::Help, _) => {
                let help =
                    help_text(catalog, &granted.capabilities, granted.remote.remote_approval);
                Ok(said(help, current))
            }
            (Remote::Decide(wanted), _) => self.decide(asked, wanted).await,
            (Remote::Sessions, _) => self.sessions_listed(agent, asked),
            (Remote::Switch, _) => self.switch(agent, asked).await,
            (Remote::New, _) => self.new_session(agent, asked).await,
            (Remote::Plan | Remote::Goal | Remote::Init, _) => self.start_turn(asked, remote).await,
            (
                Remote::Status
                | Remote::Model
                | Remote::Permission
                | Remote::Stop
                | Remote::Compact,
                None,
            ) => Ok(said(NO_SESSION, None)),
            (Remote::Status, Some(session)) => self.status(agent, asked, session).await,
            (Remote::Model, Some(session)) => self.model(agent, session, asked.arguments).await,
            (Remote::Permission, Some(session)) => self.permission(agent, asked, session).await,
            (Remote::Stop, Some(session)) => self.stop(agent, asked, session).await,
            (Remote::Compact, Some(session)) => self.compact(agent, asked, session).await,
        }
    }

    /// The composer's catalog for the chat, with the commands of its conversation's workspace
    /// when that root can be verified.
    async fn catalog_for(
        &self,
        agent: &Arc<AgentApplication>,
        current: Option<SessionId>,
    ) -> Vec<CommandEntry> {
        let workspace = current.and_then(|session| {
            let projection = projection(agent).ok()?;
            let found = projection.sessions.iter().find(|found| found.session_id == session)?;
            Some(found.workspace_id)
        });
        let (agent, plugins) = (Arc::clone(agent), Arc::clone(&self.plugins));
        tokio::task::spawn_blocking(move || {
            let root = workspace.and_then(|id| {
                let resolver = agent.execution.workspace.resolver(id).ok()?;
                resolver.verify_root_identity().ok()?;
                Some((id, resolver.canonical_root().to_path_buf()))
            });
            let user_root = agent.settings.paths().config_root.join("commands");
            let disabled = agent.settings.disabled_commands();
            let workspace = root.as_ref().map(|(id, root)| (*id, root.as_path()));
            command_catalog(&user_root, &disabled.keys, workspace, Some(&plugins)).commands
        })
        .await
        .unwrap_or_default()
    }

    /// The chat is owed the command's reply.
    fn owe(&self, plugin: &str, message: &Incoming, reply: Owed) {
        match reply {
            Owed::Now(text) => self.say(plugin, message, text),
            Owed::Turn { session, root, cursor } => {
                if let Ok(mut replies) = self.replies.lock() {
                    replies.wait_for_turn(
                        plugin,
                        message.chat.clone(),
                        Some(message.message_id.clone()),
                        session,
                        root,
                        cursor,
                        Instant::now(),
                    );
                }
            }
        }
    }

    /// `/plan`, `/goal` and `/init`: a turn in the chat's conversation, made when there is none,
    /// once the conversation is idle and has a model.
    async fn start_turn(&self, asked: &Asked<'_>, remote: Remote) -> Result<Done, ServiceError> {
        let (granted, message, text) = (asked.granted, asked.message, asked.arguments);
        let name = match remote {
            Remote::Plan => "plan",
            Remote::Goal => "goal",
            _ => "init",
        };
        if remote != Remote::Init && text.is_empty() {
            let what = if remote == Remote::Plan { "要做的事" } else { "目标" };
            return Ok(said(format!("/{name} 后面要写上{what}。"), asked.current));
        }
        let agent = self.agent(true)?;
        let Some(session) = self.chosen(&agent, granted, message).await? else {
            return Ok(said(NO_SCOPE_REPLY, None));
        };
        let Some(model) = model_of(&agent, session)? else {
            return Ok(said(NO_MODEL, Some(session)));
        };
        if let Some(busy) = busy(&agent, session).await? {
            return Ok(said(busy.reason(name), Some(session)));
        }
        let cursor = cursor(&agent).await?;
        let (command, sender) = (chat_command(asked), Some(sender_of(&granted.plugin, message)));
        let started = if remote == Remote::Init {
            let input = AgentInitStartRequest {
                computer_mode: None,
                session_id: session,
                model_profile_id: model,
                text: text.to_owned(),
            };
            agent.start_init_sent(command, input, sender).await
        } else {
            let mode =
                if remote == Remote::Plan { AgentTaskMode::Plan } else { AgentTaskMode::Goal };
            let input = AgentStartRequest {
                computer_mode: None,
                session_id: session,
                model_profile_id: model,
                text: text.to_owned(),
                mode,
                goal_budget: None,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                speed_mode: None,
                attachments: Vec::new(),
                references: Vec::new(),
            };
            agent.start_sent(command, input, sender).await
        };
        match started {
            Ok(run) => {
                self.remember(&granted.plugin, &message.chat, session).await;
                let root = run.turn.root_user_event_id;
                Ok(Done { reply: Owed::Turn { session, root, cursor }, session: Some(session) })
            }
            Err(error) => Ok(said(format!("没有开始：{}", failed(&error).message), Some(session))),
        }
    }

    /// `/stop`, as the composer's stop.
    async fn stop(
        &self,
        agent: &Arc<AgentApplication>,
        asked: &Asked<'_>,
        session: SessionId,
    ) -> Result<Done, ServiceError> {
        let request =
            PluginSessionRequest { plugin: asked.granted.plugin.clone(), session_id: session };
        let panel = PluginSessions::new(Arc::clone(agent), Arc::clone(&self.plugins));
        let stopped = panel.stop(request).await.map_err(|error| failed(&error))?.stopped;
        let text =
            if stopped { "已停止当前任务。" } else { "现在没有在运行的任务。" };
        Ok(said(text, Some(session)))
    }

    /// `/compact`: a compaction at the running turn's safe point, or one of its own when nothing
    /// runs, as the composer's.
    async fn compact(
        &self,
        agent: &Arc<AgentApplication>,
        asked: &Asked<'_>,
        session: SessionId,
    ) -> Result<Done, ServiceError> {
        let focus = (!asked.arguments.is_empty()).then(|| asked.arguments.to_owned());
        let command = chat_command(asked);
        let running = agent
            .store
            .read(StoreStep::operation("plugin_session.command_compact"), move |db| {
                session::load(db, session)?
                    .active_turn
                    .map(|turn| turns::load(db, turn).map(|turn| turn.agent_run_id))
                    .transpose()
            })
            .await
            .map_err(|error| failed(&error.into()))?;
        if let Some(agent_run_id) = running {
            let request = AgentCompactionRequest { session_id: session, agent_run_id, focus };
            agent.request_compaction(command, request).await.map_err(|error| failed(&error))?;
            return Ok(said("已登记压缩，会在任务的安全点执行。", Some(session)));
        }
        let agent = self.agent(true)?;
        let Some(model) = model_of(&agent, session)? else {
            return Ok(said(NO_MODEL, Some(session)));
        };
        if let Some(busy) = busy(&agent, session).await? {
            return Ok(said(busy.reason("compact"), Some(session)));
        }
        let cursor = cursor(&agent).await?;
        let sender = Some(sender_of(&asked.granted.plugin, asked.message));
        let input = AgentCompactionStartRequest {
            computer_mode: None,
            session_id: session,
            model_profile_id: model,
            focus,
        };
        match agent.start_compaction_sent(command, input, sender).await {
            Ok(run) => {
                let root = run.turn.root_user_event_id;
                Ok(Done { reply: Owed::Turn { session, root, cursor }, session: Some(session) })
            }
            Err(error) => Ok(said(format!("没有开始：{}", failed(&error).message), Some(session))),
        }
    }
}

/// The one command a chat's command message is, however often the platform sends it.
fn chat_command(asked: &Asked<'_>) -> CommandInvocationId {
    command_of("aura plugin chat command v1", &message_parts(&asked.granted.plugin, asked.message))
}

fn model_of(
    agent: &AgentApplication,
    session: SessionId,
) -> Result<Option<ModelProfileId>, ServiceError> {
    let binding = agent.execution.persistence.session_model_binding(session);
    Ok(binding.map_err(|error| failed(&error.into()))?.map(|bound| bound.model_profile_id))
}

/// A turn under way, or messages waiting for one.
async fn busy(
    agent: &Arc<AgentApplication>,
    session: SessionId,
) -> Result<Option<Busy>, ServiceError> {
    let inbox = agent.inbox(AgentSessionRequest { session_id: session }).await;
    let queued = !inbox.map_err(|error| failed(&error))?.messages.is_empty();
    let running = agent
        .store
        .read(StoreStep::operation("plugin_session.command_busy"), move |db| {
            Ok(session::load(db, session)?.active_turn.is_some())
        })
        .await
        .map_err(|error| failed(&error.into()))?;
    Ok(if running {
        Some(Busy::Running)
    } else if queued {
        Some(Busy::Queued)
    } else {
        None
    })
}

/// Where the journal is now: a turn started after this is read from here.
async fn cursor(agent: &AgentApplication) -> Result<u64, ServiceError> {
    agent
        .store
        .read(StoreStep::operation("plugin_session.command_cursor"), plugin_session::head)
        .await
        .map_err(|error| failed(&error.into()))
}
