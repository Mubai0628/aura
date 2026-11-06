//! The replies Aura owes a plugin's chats: one waits for the turn that took its message to end,
//! another is ready to send now. They are kept in memory only, a bounded number for a bounded
//! time; after Core restarts nothing is owed.
use aura_agent_domain::{AgentTurnOutcome, PluginChat};
use aura_domain_common::{EventId, SessionId, TurnTaskId};
use aura_plugin_host::ReplyFormat;
use std::time::{Duration, Instant};

/// The most replies one plugin's chats are owed at once; the oldest goes first.
pub const PENDING_REPLIES_MAX: usize = 100;
/// How long a reply is owed.
pub const PENDING_REPLY_TIME: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Waiting {
    /// For the turn that takes `message` in `session` to end: read from `cursor` on, `turn` once
    /// it started.
    Turn {
        session: SessionId,
        message: EventId,
        cursor: u64,
        turn: Option<TurnTaskId>,
    },
    Ready {
        text: String,
        format: ReplyFormat,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pending {
    pub id: u64,
    pub plugin: String,
    pub chat: PluginChat,
    /// The platform's own id of the message the reply answers.
    pub reply_to: Option<String>,
    pub waiting: Waiting,
    pub since: Instant,
}

#[derive(Default)]
pub struct PendingReplies {
    next: u64,
    owed: Vec<Pending>,
}

impl PendingReplies {
    fn push(
        &mut self,
        plugin: &str,
        chat: PluginChat,
        reply_to: Option<String>,
        waiting: Waiting,
        now: Instant,
    ) {
        if self.count(plugin) >= PENDING_REPLIES_MAX
            && let Some(oldest) = self.owed.iter().position(|owed| owed.plugin == plugin)
        {
            self.owed.remove(oldest);
        }
        self.next += 1;
        let id = self.next;
        self.owed.push(Pending {
            id,
            plugin: plugin.to_owned(),
            chat,
            reply_to,
            waiting,
            since: now,
        });
    }

    /// A reply owed once the turn that takes `message` ended.
    #[allow(clippy::too_many_arguments)]
    pub fn wait_for_turn(
        &mut self,
        plugin: &str,
        chat: PluginChat,
        reply_to: Option<String>,
        session: SessionId,
        message: EventId,
        cursor: u64,
        now: Instant,
    ) {
        let waiting = Waiting::Turn { session, message, cursor, turn: None };
        self.push(plugin, chat, reply_to, waiting, now);
    }

    /// A reply of Aura's own, to send as soon as the service can take it.
    pub fn ready(
        &mut self,
        plugin: &str,
        chat: PluginChat,
        reply_to: Option<String>,
        text: String,
        now: Instant,
    ) {
        self.push(plugin, chat, reply_to, Waiting::Ready { text, format: ReplyFormat::Text }, now);
    }

    #[must_use]
    pub fn count(&self, plugin: &str) -> usize {
        self.owed.iter().filter(|owed| owed.plugin == plugin).count()
    }

    /// What is owed now.
    #[must_use]
    pub fn owed(&self) -> Vec<Pending> {
        self.owed.clone()
    }

    /// The reply waits on from `cursor`, for `turn` once that took its message.
    pub fn advanced(&mut self, id: u64, to: u64, started: Option<TurnTaskId>) {
        if let Some(Pending { waiting: Waiting::Turn { cursor, turn, .. }, .. }) =
            self.owed.iter_mut().find(|owed| owed.id == id)
        {
            *cursor = to;
            if started.is_some() {
                *turn = started;
            }
        }
    }

    /// The turn ended: the reply is ready.
    pub fn resolved(&mut self, id: u64, text: String, format: ReplyFormat) {
        if let Some(owed) = self.owed.iter_mut().find(|owed| owed.id == id) {
            owed.waiting = Waiting::Ready { text, format };
        }
    }

    /// The service took it.
    pub fn sent(&mut self, id: u64) {
        self.owed.retain(|owed| owed.id != id);
    }

    /// Replies owed longer than [`PENDING_REPLY_TIME`] are no longer owed.
    pub fn expire(&mut self, now: Instant) {
        self.owed.retain(|owed| now.duration_since(owed.since) < PENDING_REPLY_TIME);
    }

    /// A plugin's service is gone for good: nothing more is owed to its chats.
    pub fn forget(&mut self, plugin: &str) {
        self.owed.retain(|owed| owed.plugin != plugin);
    }
}

/// What the chat is told once the turn ended: its final answer, or how it ended.
#[must_use]
pub fn reply_for(
    outcome: Option<AgentTurnOutcome>,
    final_reply: Option<&str>,
) -> (String, ReplyFormat) {
    let text = |said: &str| (said.to_owned(), ReplyFormat::Text);
    match (outcome, final_reply.filter(|reply| !reply.trim().is_empty())) {
        (Some(AgentTurnOutcome::Completed), Some(reply)) => {
            (reply.to_owned(), ReplyFormat::Markdown)
        }
        (Some(AgentTurnOutcome::Completed), None) => text("这一轮完成了，没有回复。"),
        (Some(AgentTurnOutcome::Failed), _) => text("这一轮出错了，没有完成。"),
        (Some(AgentTurnOutcome::Cancelled), _) => text("这一轮被停止了。"),
        (Some(AgentTurnOutcome::RecoveryRequired) | None, _) => {
            text("这一轮停下了，要在 Aura 里恢复后才能继续。")
        }
    }
}
