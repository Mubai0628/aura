//! The replies owed to the plugins' chats, sent once a second. How the turn that took each
//! message went is read from the journal past the reply's cursor; a reply that is ready goes to
//! its plugin's service in the pieces its channel takes, and one its service did not take whole
//! goes again, whole, once the service runs again, while the reply is still owed. An approval
//! that turn waits for is offered to the chat, and handed out after (`approvals`).
use super::driver::PluginServices;
use crate::channels::{ChatApproval, Pending, Waiting, outgoing, reply_for};
use aura_agent_domain::{PluginChat, PluginSessionEventKind};
use aura_core_broker::agent_store::StoreStep;
use aura_domain_common::{EventId, SessionId, TurnTaskId};
use aura_persistence_core::{PersistenceCoreError, StoreResult};
use aura_persistence_v2::agent::{inbox, plugin_session, turns};
use aura_plugin_host::{ChatAddress, ReplyFormat, ToService};
use rusqlite::Connection;
use std::time::{Duration, Instant};

pub(super) const PUMP_EVERY: Duration = Duration::from_secs(1);
/// How many windows of the journal one reply reads on at most each time.
const WINDOWS_AT_ONCE: usize = 20;

struct Wait {
    id: u64,
    plugin: String,
    chat: PluginChat,
    session: SessionId,
    message: EventId,
    cursor: u64,
    turn: Option<TurnTaskId>,
}

struct Progress {
    id: u64,
    plugin: String,
    cursor: u64,
    turn: Option<TurnTaskId>,
    ended: Option<(String, ReplyFormat)>,
    /// The approvals the turn began to wait for, in the order it did.
    asked: Vec<ChatApproval>,
}

impl PluginServices {
    /// Reads on for the replies that wait for a turn, hands every ready one to its service, then
    /// the approvals their turns wait for.
    pub(super) async fn pump(&self) {
        let sessions = self.bridge().sessions();
        let waiting: Vec<Wait> = {
            let Ok(mut replies) = sessions.replies.lock() else { return };
            replies.expire(Instant::now());
            let owed = replies.owed();
            owed.into_iter()
                .filter_map(|owed| match owed.waiting {
                    Waiting::Turn { session, message, cursor, turn } => Some(Wait {
                        id: owed.id,
                        plugin: owed.plugin,
                        chat: owed.chat,
                        session,
                        message,
                        cursor,
                        turn,
                    }),
                    Waiting::Ready { .. } => None,
                })
                .collect()
        };
        if !waiting.is_empty()
            && let Some(agent) = sessions.reading()
            && let Ok(progress) = agent
                .store
                .read(StoreStep::operation("plugin_session.replies"), move |db| {
                    waiting.iter().map(|wait| read_on(db, wait)).collect::<StoreResult<Vec<_>>>()
                })
                .await
            && let Ok(mut replies) = sessions.replies.lock()
        {
            for step in progress {
                replies.advanced(step.id, step.cursor, step.turn);
                if let Some((text, format)) = step.ended {
                    replies.resolved(step.id, text, format);
                }
                if !step.asked.is_empty()
                    && let Ok(mut approvals) = sessions.approvals.lock()
                {
                    for asked in step.asked {
                        approvals.offer(&step.plugin, asked);
                    }
                }
            }
        }
        let ready: Vec<Pending> =
            sessions.replies.lock().map(|replies| replies.owed()).unwrap_or_default();
        for owed in ready {
            let Waiting::Ready { text, format } = &owed.waiting else { continue };
            let channels = self.bridge().channels(&owed.plugin);
            let channel = channels.iter().find(|channel| channel.id == owed.chat.channel);
            // A service that could not take a piece is ended, and the pieces it took may never
            // have reached the chat: the reply goes again, whole, to the one started after it.
            if self.deliver(&owed, outgoing(text, *format, channel))
                && let Ok(mut replies) = sessions.replies.lock()
            {
                replies.sent(owed.id);
            }
        }
        self.hand_approvals().await;
    }

    /// Hands the reply's pieces to the service in order, numbered when there are several; whether
    /// it took them all.
    fn deliver(&self, owed: &Pending, pieces: Vec<(String, ReplyFormat)>) -> bool {
        let parts = u32::try_from(pieces.len()).unwrap_or(u32::MAX);
        pieces.into_iter().enumerate().all(|(at, (text, format))| {
            let reply = ToService::Reply {
                channel: owed.chat.channel.clone(),
                chat: ChatAddress { kind: owed.chat.kind, id: owed.chat.id.clone() },
                reply_to: owed.reply_to.clone(),
                text,
                format,
                part: (parts > 1).then(|| u32::try_from(at + 1).unwrap_or(u32::MAX)),
                parts: (parts > 1).then_some(parts),
            };
            self.host().deliver(&owed.plugin, &reply).is_ok()
        })
    }
}

/// What happened in the reply's conversation since its cursor: the turn that took its message,
/// the approvals it began to wait for, and how it ended.
fn read_on(db: &Connection, wait: &Wait) -> StoreResult<Progress> {
    let (mut cursor, mut turn) = (wait.cursor, wait.turn);
    let mut asked = Vec::new();
    let progress = |cursor, turn, ended, asked| Progress {
        id: wait.id,
        plugin: wait.plugin.clone(),
        cursor,
        turn,
        ended,
        asked,
    };
    for _ in 0..WINDOWS_AT_ONCE {
        let read = plugin_session::events(db, wait.session, cursor, plugin_session::EVENTS_WINDOW)?;
        for event in read.events {
            match event.event {
                PluginSessionEventKind::TurnStarted { turn_task_id } => {
                    if turn.is_none() && took(db, turn_task_id, wait.message)? {
                        turn = Some(turn_task_id);
                    }
                }
                PluginSessionEventKind::ApprovalWaiting { turn_task_id, approval_id }
                    if turn == Some(turn_task_id) =>
                {
                    let chat = wait.chat.clone();
                    asked.push(ChatApproval { approval: approval_id, session: wait.session, chat });
                }
                PluginSessionEventKind::TurnEnded {
                    turn_task_id, outcome, final_reply, ..
                } if turn == Some(turn_task_id) => {
                    let ended = Some(reply_for(outcome, final_reply.as_deref()));
                    return Ok(progress(event.sequence, turn, ended, asked));
                }
                _ => {}
            }
        }
        cursor = read.cursor;
        if !read.more {
            break;
        }
    }
    Ok(progress(cursor, turn, None, asked))
}

/// The turn took the message: it began with it, or took it with the others waiting.
fn took(db: &Connection, turn: TurnTaskId, message: EventId) -> StoreResult<bool> {
    match turns::load(db, turn) {
        Ok(task) if task.root_user_event_id == message => return Ok(true),
        Ok(_) => {}
        Err(PersistenceCoreError::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => {
            return Ok(false);
        }
        Err(error) => return Err(error),
    }
    let consumed = inbox::consumed(db, turn)?;
    Ok(consumed.iter().any(|input| input.user_event_ids.contains(&message)))
}
