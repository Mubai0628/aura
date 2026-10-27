//! A stored context event as `read_events` returns it: its whole text, so none of the fields that
//! name a part of it are set.
use aura_context_domain::ContextReadEventView;
use aura_persistence::ContextEventStoredRow;

pub(crate) fn whole(row: ContextEventStoredRow) -> ContextReadEventView {
    ContextReadEventView {
        event_id: row.event_id,
        session_sequence: row.session_sequence,
        event_type: row.event_type,
        actor: row.actor,
        history_atomic_group_ref: row.history_atomic_group_ref,
        atomic_group_id: row.atomic_group_id,
        legacy_atomic_group_verification: row.legacy_atomic_group_verification,
        content: row.model_visible_content,
        content_hash: row.content_hash,
        content_chars: None,
        content_offset: None,
        content_next_offset: None,
    }
}
