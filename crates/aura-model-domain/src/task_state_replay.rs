//! A Core-authored checkpoint and the complete state replacements already sent after it.
use aura_domain_common::{ContentHash, EventId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{LogicalConversationItem, logical_request::is_canonical_instruction_text};

/// Scoped to the admitted conversation and to the sources still authorized for that conversation.
/// An update follows a whole exchange, so replay cannot split a function call and its results.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskStateReplay {
    pub authority: ContentHash,
    pub active_sources: Vec<ContentHash>,
    pub leading: String,
    pub updates: Vec<TaskStateUpdate>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskStateUpdate {
    pub after: EventId,
    pub content: String,
}

impl TaskStateReplay {
    #[must_use]
    pub fn current(&self) -> &str {
        self.updates.last().map_or(&self.leading, |update| &update.content)
    }

    pub fn messages(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.leading.as_str())
            .chain(self.updates.iter().map(|update| update.content.as_str()))
    }

    #[must_use]
    pub fn is_authorized_by(&self, current: &Self) -> bool {
        self.authority == current.authority
            && self.active_sources.iter().all(|source| {
                current
                    .active_sources
                    .binary_search_by(|known| known.hex.cmp(&source.hex))
                    .is_ok_and(|index| current.active_sources[index] == *source)
            })
    }

    #[must_use]
    pub fn fits(&self, items: &[LogicalConversationItem]) -> bool {
        if !self.active_sources.windows(2).all(|pair| pair[0].hex < pair[1].hex)
            || !is_canonical_instruction_text(&self.leading)
        {
            return false;
        }
        let mut from = 0;
        let mut latest = self.leading.as_str();
        for update in &self.updates {
            let Some(at) =
                items[from..].iter().position(|item| item.source_event_id() == update.after)
            else {
                return false;
            };
            from += at + 1;
            if matches!(items.get(from), Some(LogicalConversationItem::ToolResult { .. }))
                || !is_canonical_instruction_text(&update.content)
                || latest == update.content
            {
                return false;
            }
            latest = &update.content;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::ToolCallId;

    fn record(content: &str) -> LogicalConversationItem {
        LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: content.to_owned(),
            content_hash: ContentHash::sha256(content.as_bytes()),
        }
    }

    #[test]
    fn more_than_thirty_two_changes_keep_the_baseline_across_a_snapshot_round_trip() {
        let items = (0..48).map(|i| record(&format!("source {i}"))).collect::<Vec<_>>();
        let replay = TaskStateReplay {
            authority: ContentHash::sha256(b"session model scope"),
            active_sources: vec![],
            leading: "original state".to_owned(),
            updates: items
                .iter()
                .enumerate()
                .map(|(i, item)| TaskStateUpdate {
                    after: item.source_event_id(),
                    content: format!("state update {i}"),
                })
                .collect(),
        };
        let restored: TaskStateReplay =
            serde_json::from_slice(&serde_json::to_vec(&replay).expect("serialize replay"))
                .expect("restore replay");
        assert!(restored.fits(&items));
        assert_eq!(restored, replay);
        assert_eq!(restored.current(), "state update 47");
        assert!(!restored.fits(&items[1..]));
    }

    #[test]
    fn cannot_split_a_tool_batch_or_re_admit_a_revoked_source() {
        let input = record("user");
        let result = LogicalConversationItem::ToolResult {
            source_event_id: EventId::new(),
            tool_call_id: ToolCallId::new(),
            tool_name: "fixture.read".to_owned(),
            content: "ok".to_owned(),
            content_hash: ContentHash::sha256(b"ok"),
            is_error: false,
        };
        let source = ContentHash::sha256(b"original source");
        let mut replay = TaskStateReplay {
            authority: ContentHash::sha256(b"scope"),
            active_sources: vec![source],
            leading: "baseline".to_owned(),
            updates: vec![TaskStateUpdate {
                after: input.source_event_id(),
                content: "update".to_owned(),
            }],
        };
        assert!(!replay.fits(&[input.clone(), result.clone()]));
        replay.updates[0].after = result.source_event_id();
        assert!(replay.fits(&[input, result]));
        let mut revoked = replay.clone();
        revoked.active_sources.clear();
        assert!(!replay.is_authorized_by(&revoked));
        revoked.active_sources.clone_from(&replay.active_sources);
        revoked.authority = ContentHash::sha256(b"changed permission");
        assert!(!replay.is_authorized_by(&revoked));
    }
}
