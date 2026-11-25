//! Markdown extension properties stay opaque to older Aura versions. They never
//! change the strict audit JSON or allow a tool to inject actor/session identity.
use crate::{
    MemoryAuthorStamp, MemoryDocument, MemoryHistory, MemoryProvenance, MemoryResult, WriteActor,
};
use aura_domain_common::UtcTimestamp;

const CREATED: &str = "aura_creation_stamp:";
const MODIFIED: &str = "aura_modification_stamps:";

fn decode(value: &str) -> Option<MemoryAuthorStamp> {
    let scalar: String = serde_json::from_str(value.trim()).ok()?;
    serde_json::from_str(&scalar).ok()
}

fn scalar(stamp: &MemoryAuthorStamp) -> MemoryResult<String> {
    serde_json::to_string(stamp)
        .and_then(|value| serde_json::to_string(&value))
        .map_err(|_| crate::MemoryError::Invalid("provenance stamp"))
}

fn authored(property: &str) -> bool {
    property.starts_with(CREATED) || property.starts_with(MODIFIED)
}

pub(crate) fn stamps(note: &MemoryDocument) -> MemoryProvenance {
    let mut result = MemoryProvenance::default();
    for property in &note.metadata.user_properties {
        if let Some(value) = property.strip_prefix(CREATED) {
            result.creator = decode(value);
        } else if let Some(values) = property.strip_prefix(MODIFIED) {
            result
                .modifiers
                .extend(values.lines().filter_map(|line| decode(line.trim().strip_prefix("- ")?)));
        }
    }
    result
}

pub(crate) fn event_stamp(h: &MemoryHistory) -> MemoryAuthorStamp {
    MemoryAuthorStamp {
        actor: h.actor.clone(),
        session_id: h.session_id,
        session_name_at_write: None,
        at: h.at,
    }
}

fn matches_event(stamp: &MemoryAuthorStamp, history: &MemoryHistory) -> bool {
    stamp.actor == history.actor && stamp.session_id == history.session_id && stamp.at == history.at
}

pub(crate) fn view(
    note: &MemoryDocument,
    creation: Option<&MemoryHistory>,
    recent: &[MemoryHistory],
) -> MemoryProvenance {
    let mut recorded = stamps(note);
    // An audit create row is a sufficient legacy source. A list of sessions that
    // contributed later is not. External creation can never claim an Aura actor.
    recorded.creator = creation.map(|h| {
        recorded.creator.filter(|stamp| matches_event(stamp, h)).unwrap_or_else(|| event_stamp(h))
    });
    for h in recent.iter().rev().filter(|h| h.action != "create" && h.action != "external_create") {
        if !recorded.modifiers.iter().any(|s| matches_event(s, h)) {
            recorded.modifiers.push(event_stamp(h));
        }
    }
    recorded.modifiers.sort_by_key(|s| s.at);
    recorded
}

pub(crate) fn stamp(
    note: &mut MemoryDocument,
    previous: Option<MemoryProvenance>,
    actor: &WriteActor,
    at: UtcTimestamp,
) -> MemoryResult<()> {
    note.metadata.user_properties.retain(|p| !authored(p));
    let stamp = MemoryAuthorStamp {
        actor: actor.kind.clone(),
        session_id: actor.session_id,
        session_name_at_write: actor.session_name.as_deref().and_then(|name| {
            let bounded = name.chars().take(160).collect::<String>();
            aura_secret_scanner::detect_text(&bounded).is_none().then_some(bounded)
        }),
        at,
    };
    let provenance = previous.map_or_else(
        || MemoryProvenance { creator: Some(stamp.clone()), modifiers: Vec::new() },
        |mut previous| {
            previous.modifiers.push(stamp.clone());
            previous
        },
    );
    if let Some(creator) = provenance.creator {
        note.metadata.user_properties.push(format!("{CREATED} {}", scalar(&creator)?));
    }
    if !provenance.modifiers.is_empty() {
        let mut property = MODIFIED.to_owned();
        for modifier in provenance.modifiers {
            property.push_str("\n  - ");
            property.push_str(&scalar(&modifier)?);
        }
        note.metadata.user_properties.push(property);
    }
    Ok(())
}
