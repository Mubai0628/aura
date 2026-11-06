//! `message.in`: what one of the plugin's channels received. Someone not on the allowlist does
//! not reach Aura and is listed on the plugin page; a message that starts with a command runs it
//! (`commands`); anyone else's message goes to the chat's conversation inside the scope, marked
//! with who sent it, and a reply is owed once the turn that takes it ended. A plugin's messages
//! are taken one at a time in the order its service sent them, so a chat's messages reach the
//! conversation in that order and a new chat gets one conversation, not one for each message.
use super::bridge::failed;
use super::sessions::{Granted, ServiceSessions, create, open_now, projection};
use crate::channels::{Choice, Incoming, Known, choose, incoming, typed};
use crate::checks::command_of;
use aura_agent_application::agent_application::AgentApplication;
use aura_agent_domain::{
    AgentInputLane, AgentSubmitRequest, MessageFrom, MessageSender, PluginChat, PluginChatKind,
    PluginLogSource,
};
use aura_core_broker::agent_store::StoreStep;
use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::agent::{inbox, plugin_session};
use aura_plugin_host::{CallFuture, MAX_IN_FLIGHT, ServiceError, shown};
use aura_plugins::PluginUnknownUser;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};
use std::time::Instant;
use tokio::sync::{mpsc, oneshot};

/// What a chat is told when the plugin's scope has no conversation it can go to.
pub const NO_SCOPE_REPLY: &str =
    "这个机器人还没有可以进入的 Aura 会话：请在 Aura 的 设置 → 插件 里给它选择会话范围。";
/// Messages one plugin may have waiting to be taken: a service and the one started after it.
const WAITING_MESSAGES: usize = 2 * MAX_IN_FLIGHT;

type Job = (Granted, Value, oneshot::Sender<Result<Value, ServiceError>>);

/// Each plugin's messages, waiting to be taken in order.
#[derive(Default)]
pub(crate) struct InOrder {
    queues: Mutex<BTreeMap<String, mpsc::Sender<Job>>>,
}

impl InOrder {
    /// The message, behind the plugin's earlier ones.
    pub(crate) fn take(
        &self,
        sessions: &Arc<ServiceSessions>,
        granted: Granted,
        params: Value,
    ) -> CallFuture {
        let (answer, answered) = oneshot::channel();
        let queued = self.queues.lock().ok().is_some_and(|mut queues| {
            let queue = queues.entry(granted.plugin.clone()).or_insert_with(|| taker(sessions));
            if queue.is_closed() {
                *queue = taker(sessions);
            }
            queue.try_send((granted, params, answer)).is_ok()
        });
        Box::pin(async move {
            if !queued {
                return Err(ServiceError::new(
                    "protocol.busy",
                    format!("等着交给 Aura 的消息最多 {WAITING_MESSAGES} 条，等前面的回应了再发"),
                ));
            }
            answered
                .await
                .unwrap_or_else(|_| Err(ServiceError::new("aura.stopping", "Aura 正在退出")))
        })
    }

    /// The service ended: what it sent is still taken, then its queue ends.
    pub(crate) fn end(&self, plugin: &str) {
        if let Ok(mut queues) = self.queues.lock() {
            queues.remove(plugin);
        }
    }
}

/// Takes one plugin's messages one after another.
fn taker(sessions: &Arc<ServiceSessions>) -> mpsc::Sender<Job> {
    let (queue, mut jobs) = mpsc::channel::<Job>(WAITING_MESSAGES);
    let owner: Weak<ServiceSessions> = Arc::downgrade(sessions);
    sessions.runtime.spawn(async move {
        while let Some((granted, params, answer)) = jobs.recv().await {
            let outcome = match owner.upgrade() {
                Some(sessions) => sessions.message_in(granted, params).await,
                None => Err(ServiceError::new("aura.stopping", "Aura 正在退出")),
            };
            let _ = answer.send(outcome);
        }
    });
    queue
}

pub(super) fn kind_name(kind: PluginChatKind) -> &'static str {
    match kind {
        PluginChatKind::Private => "private",
        PluginChatKind::Group => "group",
    }
}

/// What the platform's message is known by: its chat and its own id.
pub(super) fn message_parts<'a>(plugin: &'a str, message: &'a Incoming) -> [&'a str; 5] {
    let chat = &message.chat;
    [plugin, &chat.channel, kind_name(chat.kind), &chat.id, &message.message_id]
}

/// The message is marked as sent through the plugin, by whom on which platform.
pub(super) fn sender_of(plugin: &str, message: &Incoming) -> MessageSender {
    let from = MessageFrom {
        channel: message.chat.channel.clone(),
        platform: message.platform.name().to_owned(),
        chat: message.chat.kind,
        user_id: message.user_id.clone(),
        user_name: message.user_name.clone(),
    };
    MessageSender { plugin: plugin.to_owned(), from: Some(from) }
}

/// A new chat's conversation is named after its channel and who it is with.
pub(super) fn chat_title(granted: &Granted, message: &Incoming) -> String {
    let channel = granted
        .channels
        .iter()
        .find(|channel| channel.id == message.chat.channel)
        .map_or(message.chat.channel.as_str(), |channel| channel.title.as_str());
    match message.chat.kind {
        PluginChatKind::Private => {
            let name = message.user_name.trim();
            format!("{channel} · {}", if name.is_empty() { &message.user_id } else { name })
        }
        PluginChatKind::Group => format!("{channel} · 群 {}", message.chat.id),
    }
}

impl ServiceSessions {
    pub(crate) async fn message_in(
        &self,
        granted: Granted,
        params: Value,
    ) -> Result<Value, ServiceError> {
        let message = incoming(params, &granted.channels)?;
        let plugin = granted.plugin.as_str();
        if !granted.remote.allows(&message.chat.channel, &message.user_id) {
            self.not_allowed(plugin, &message).await;
            return Ok(json!({"accepted": false, "reason": "user_not_allowed"}));
        }
        if granted.remote.scope.is_empty() {
            return Ok(self.no_scope(plugin, &message));
        }
        if let Some(typed) = typed(&message.text)
            && let Some(answer) = self.command_in(&granted, &message, typed).await?
        {
            return Ok(answer);
        }
        self.deliver_message(&granted, &message, &message.text).await
    }

    /// `text` to the chat's conversation inside the scope as the platform's message, marked with
    /// who sent it; the turn that takes it owes the chat a reply.
    pub(super) async fn deliver_message(
        &self,
        granted: &Granted,
        message: &Incoming,
        text: &str,
    ) -> Result<Value, ServiceError> {
        let plugin = granted.plugin.as_str();
        let agent = self.agent(true)?;
        let Some(session) = self.chosen(&agent, granted, message).await? else {
            return Ok(self.no_scope(plugin, message));
        };
        let chat = &message.chat;
        let command = command_of("aura plugin channel message v1", &message_parts(plugin, message));
        let submit = AgentSubmitRequest::new(session, text, AgentInputLane::NextTurn);
        let asked = submit.clone();
        // The same message from the platform again, even in another conversation by now, is the
        // same message.
        let (before, cursor) = agent
            .store
            .read(StoreStep::operation("plugin_session.message_in"), move |db| {
                let before = match inbox::submitted(db, command, &asked) {
                    Err(PersistenceCoreError::Conflict) => true,
                    other => other?,
                };
                Ok((before, plugin_session::head(db)?))
            })
            .await
            .map_err(|error| failed(&error.into()))?;
        let repeated =
            json!({"accepted": true, "session_id": session, "queued": false, "repeated": true});
        if before {
            return Ok(repeated);
        }
        let sender = sender_of(plugin, message);
        let (inbox, sent) = agent
            .submit_sent(command, submit, Some(sender))
            .await
            .map_err(|error| failed(&error))?;
        let Some(sent) = sent else { return Ok(repeated) };
        self.remember(plugin, chat, session).await;
        if let Ok(mut replies) = self.replies.lock() {
            let reply_to = Some(message.message_id.clone());
            replies.wait_for_turn(
                plugin,
                chat.clone(),
                reply_to,
                session,
                sent,
                cursor,
                Instant::now(),
            );
        }
        Ok(json!({
            "accepted": true,
            "session_id": session,
            "message_id": sent,
            "queued": inbox.messages.iter().any(|queued| queued.message_id == sent),
            "repeated": false,
        }))
    }

    /// Where the chat's next message goes now, without making anything.
    pub(super) async fn choice(
        &self,
        agent: &AgentApplication,
        granted: &Granted,
        message: &Incoming,
    ) -> Result<Choice, ServiceError> {
        let (plugins, plugin, chat) =
            (Arc::clone(&self.plugins), granted.plugin.clone(), message.chat.clone());
        let (current, used) =
            tokio::task::spawn_blocking(move || plugins.chat_sessions(&plugin, &chat))
                .await
                .unwrap_or_default();
        let projection = projection(agent)?;
        let (workspaces, sessions) = open_now(&projection);
        let sessions: Vec<(SessionId, WorkspaceId)> =
            sessions.iter().map(|session| (session.session_id, session.workspace_id)).collect();
        let known = Known { sessions: &sessions, workspaces: &workspaces };
        Ok(choose(&granted.remote.scope, &known, current, &used))
    }

    /// The conversation the chat's message goes to, made when there is none; none when the scope
    /// has nowhere for it.
    pub(super) async fn chosen(
        &self,
        agent: &Arc<AgentApplication>,
        granted: &Granted,
        message: &Incoming,
    ) -> Result<Option<SessionId>, ServiceError> {
        match self.choice(agent, granted, message).await? {
            Choice::Use(session) => Ok(Some(session)),
            Choice::NoScope => Ok(None),
            Choice::Create(workspace) => {
                // The same message again finds the conversation it made.
                let command = command_of(
                    "aura plugin chat session v1",
                    &message_parts(&granted.plugin, message),
                );
                let title = chat_title(granted, message);
                Ok(Some(create(agent, workspace, title, command).await?.session_id))
            }
        }
    }

    /// The chat is told where the user gives the plugin conversations to go to.
    fn no_scope(&self, plugin: &str, message: &Incoming) -> Value {
        self.say(plugin, message, NO_SCOPE_REPLY.into());
        json!({"accepted": false, "reason": "no_scope"})
    }

    /// A reply of Aura's own to the message, sent as soon as the service can take it.
    pub(super) fn say(&self, plugin: &str, message: &Incoming, text: String) {
        if let Ok(mut replies) = self.replies.lock() {
            let reply_to = Some(message.message_id.clone());
            replies.ready(plugin, message.chat.clone(), reply_to, text, Instant::now());
        }
    }

    pub(super) async fn remember(&self, plugin: &str, chat: &PluginChat, session: SessionId) {
        let (plugins, plugin, chat) = (Arc::clone(&self.plugins), plugin.to_owned(), chat.clone());
        let _ = tokio::task::spawn_blocking(move || plugins.remember_chat(&plugin, &chat, session))
            .await;
    }

    /// Listed on the plugin page; the plugin's log says so the first time they are listed.
    async fn not_allowed(&self, plugin: &str, message: &Incoming) {
        let user = PluginUnknownUser {
            channel: message.chat.channel.clone(),
            id: message.user_id.clone(),
            name: message.user_name.clone(),
            chat: message.chat.kind,
            at: UtcTimestamp::now(),
        };
        let line = format!(
            "渠道 {} 上不在白名单里的用户 {} 发来消息，没有交给 Aura；在 设置 → 插件 里可以把这个用户加进白名单",
            message.chat.channel,
            shown(&message.user_id)
        );
        let (plugins, plugin) = (Arc::clone(&self.plugins), plugin.to_owned());
        let _ = tokio::task::spawn_blocking(move || {
            if plugins.note_unknown(&plugin, user) {
                plugins.log(&plugin, PluginLogSource::Service, "message.in", &line);
            }
        })
        .await;
    }
}
