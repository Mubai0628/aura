//! Compaction anchors name one frozen history item, not the user-message body.
//!
//! The lineage source keeps BLAKE3 of the original user text. A history item
//! keeps SHA-256 of the text it actually emitted. The selector's anchor is
//! SHA-256 of that whole item. Callers compare an anchor only to this hash.

use crate::ContextReadCompactionResult;
use aura_domain_common::{ContentHash, EventId};
use aura_model_domain::LogicalConversationItem;

pub fn conversation_item_anchor_hash(
    item: &LogicalConversationItem,
) -> Result<ContentHash, serde_json::Error> {
    Ok(ContentHash::sha256(serde_json::to_string(item)?.as_bytes()))
}

/// The user item `history_for_model` emits: the stored text, hashed as text.
pub fn user_history_anchor_hash(
    event_id: EventId,
    text: &str,
) -> Result<ContentHash, serde_json::Error> {
    conversation_item_anchor_hash(&LogicalConversationItem::User {
        source_event_id: event_id,
        content: text.to_owned(),
        content_hash: ContentHash::sha256(text.as_bytes()),
    })
}

pub fn compaction_result_anchor_hash(
    result: &ContextReadCompactionResult,
) -> Result<ContentHash, serde_json::Error> {
    Ok(ContentHash::sha256(serde_json::to_string(result)?.as_bytes()))
}

pub fn emitted_text_hash(text: &str) -> ContentHash {
    ContentHash::sha256(text.as_bytes())
}

/// An anchor matches this stored user text only when it is the selector hash
/// of the history item that text would emit.
pub fn anchor_matches_user_history(
    anchor_hash: &ContentHash,
    event_id: EventId,
    text: &str,
) -> bool {
    user_history_anchor_hash(event_id, text).is_ok_and(|expected| expected == *anchor_hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_history_item_anchor_is_not_the_source_body_hash() {
        let text = "Verify the requested work";
        let event_id = EventId::new();
        let anchor = user_history_anchor_hash(event_id, text).expect("item");
        assert_ne!(anchor, ContentHash::blake3(text.as_bytes()));
        assert_ne!(anchor, ContentHash::sha256(text.as_bytes()));
        let item = LogicalConversationItem::User {
            source_event_id: event_id,
            content: text.to_owned(),
            content_hash: emitted_text_hash(text),
        };
        assert_eq!(anchor, conversation_item_anchor_hash(&item).expect("same item"));
    }
}
