//! Another conversation's history, read for one whose user message referred to it with "@". It is
//! found by its handle among the workspace's conversations, and what is read from it names it by
//! that handle, so nothing the reading conversation keeps names the other by its id: deleting a
//! Session finds its records by its id.
use aura_context_domain::HistoryEntryKind;
use aura_domain_common::{ConversationHandle, SessionId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;

/// The rounds a recap names, and how much of the last reply it keeps.
const RECAP_ROUNDS: usize = 5;
const RECAP_REPLY_CHARS: usize = 400;

/// The other conversation a history call names, when it names one other than `session`.
pub(super) fn other(
    db: &Connection,
    session: SessionId,
    named: Option<&ConversationHandle>,
) -> StoreResult<Option<SessionId>> {
    let Some(named) = named.filter(|named| **named != ConversationHandle::of(session)) else {
        return Ok(None);
    };
    if !crate::agent::references::refers_to(db, session, named)? {
        return Err(Error::Invalid(
            "history.conversation_not_referred: only a conversation a user message of this one \
             referred to with @ can be read",
        ));
    }
    let found = match crate::workspace::session(db, session)? {
        Some(this) => crate::workspace::sessions_of(db, this.workspace_id)?
            .into_iter()
            .find(|id| ConversationHandle::of(*id) == *named),
        None => None,
    };
    found.map(Some).ok_or(Error::Invalid(
        "history.conversation_unavailable: the referred conversation was deleted or has no \
         messages",
    ))
}

/// What another conversation's history shows, with that conversation named by its handle.
pub(super) fn named_by_handle(
    output: serde_json::Value,
    other: SessionId,
) -> StoreResult<serde_json::Value> {
    let text = serde_json::to_string(&output)?;
    let id = other.to_string();
    if !text.contains(&id) {
        return Ok(output);
    }
    Ok(serde_json::from_str(&text.replace(&id, ConversationHandle::of(other).as_str()))?)
}

/// What a conversation was about, for another that refers to it: its latest summary when it has
/// one, or else its last rounds' first messages and the start of its last reply, one line each,
/// oldest first, naming the conversation itself by its handle.
pub fn recap_lines(db: &Connection, session: SessionId) -> StoreResult<Vec<String>> {
    let (id, handle) = (session.to_string(), ConversationHandle::of(session));
    let named = |line: String| line.replace(&id, handle.as_str());
    if let Some(summary) = crate::context::compaction::latest(db, session)?
        .and_then(|compaction| compaction.result)
        .map(|result| result.historical_snapshot)
        .filter(|summary| !summary.trim().is_empty())
    {
        return Ok(vec![named(summary)]);
    }
    let all = super::rounds(db, session)?;
    let mut lines: Vec<String> = all
        .iter()
        .skip(all.len().saturating_sub(RECAP_ROUNDS))
        .map(|round| {
            let shown = super::line(round);
            format!("{} ({}): {}", shown.id, shown.sent_at, shown.first_message)
        })
        .collect();
    if let Some(round) = all.last() {
        let entries = super::entries::of(&super::rows::Reader::new(db, session).rows(round)?);
        if let Some(reply) = entries.iter().rev().find(|e| e.kind == HistoryEntryKind::ModelOutput)
        {
            let mut start: String = reply.text.chars().take(RECAP_REPLY_CHARS).collect();
            if reply.text.chars().nth(RECAP_REPLY_CHARS).is_some() {
                start.push('…');
            }
            lines.push(format!("Last reply: {start}"));
        }
    }
    Ok(lines.into_iter().map(named).collect())
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{ConversationHandle, SessionId};

    #[test]
    fn what_is_read_from_another_conversation_names_it_by_its_handle() {
        let other = SessionId::new();
        let read = serde_json::json!({"text": format!("see {other} and {other}"), "n": 1});
        let named = super::named_by_handle(read, other).expect("named");
        let handle = ConversationHandle::of(other);
        assert_eq!(
            named,
            serde_json::json!({"text": format!("see {handle} and {handle}"), "n": 1})
        );
        let plain = serde_json::json!({"text": "nothing to name"});
        assert_eq!(super::named_by_handle(plain.clone(), other).expect("same"), plain);
    }
}
