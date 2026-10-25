//! Whether a frozen request still covers one stored lineage source.
//!
//! A source body is the user message's BLAKE3. A history item is SHA-256 of
//! its emitted text. A compaction anchor is the selector hash of that whole
//! item. Coverage uses those stored facts; a summary string is not an identity.

use super::session;
use aura_agent_domain::TaskLineageSource;
use aura_context_domain::{
    COVERAGE_BUDGET_ERROR, COVERAGE_WALK_BUDGET, ContextProjectionSnapshot, ContextSourceRef,
    CoverageHop, CoverageVerdict, ModelEgressAuthorizationBasis, ModelEgressManifestV1,
    ModelEgressSourceKind, SourceIdentity, emitted_text_hash, walk_coverage,
};
use aura_model_domain::LogicalConversationItem;
use aura_persistence_core::{Connection, PersistenceCoreError as Error, StoreResult, artifact};
use rusqlite::Transaction;
use std::collections::BTreeSet;

pub(super) fn source_covered(
    tx: &Transaction<'_>,
    snapshot: &ContextProjectionSnapshot,
    source: &TaskLineageSource,
) -> StoreResult<bool> {
    let user = session::user_message(tx, snapshot.session_id, source.user_event_id)?;
    if user.content_hash != source.content_hash || user.text.is_empty() {
        return Ok(false);
    }
    if snapshot_shows_exact_history_item(snapshot, source, &user.text) {
        return Ok(true);
    }
    if snapshot_manifest_binds_user_text(tx, snapshot, source, &user.text)? {
        return Ok(true);
    }
    for section in &snapshot.sections {
        for source_ref in &section.source_refs {
            let ContextSourceRef::Compaction { context_snapshot_id } = source_ref else {
                continue;
            };
            let compaction = crate::context::compaction::load(tx, *context_snapshot_id)?;
            match walk_coverage(
                hop_from(&compaction),
                |artifact_id| load_artifact_hop(tx, artifact_id),
                source.user_event_id,
                snapshot.session_id,
                SourceIdentity::StoredText(&user.text),
                COVERAGE_WALK_BUDGET,
            )?
            .verdict
            {
                CoverageVerdict::Covered => return Ok(true),
                CoverageVerdict::NotCovered => {}
                CoverageVerdict::BudgetExhausted => {
                    return Err(Error::Invalid(COVERAGE_BUDGET_ERROR));
                }
            }
        }
    }
    Ok(false)
}

fn snapshot_shows_exact_history_item(
    snapshot: &ContextProjectionSnapshot,
    source: &TaskLineageSource,
    text: &str,
) -> bool {
    let emitted = emitted_text_hash(text);
    snapshot.conversation_items.iter().any(|item| {
        let LogicalConversationItem::User { source_event_id, content, content_hash } = item else {
            return false;
        };
        *source_event_id == source.user_event_id && content == text && *content_hash == emitted
    })
}

/// The first request's user item is the compiler's joined working set. The
/// frozen egress manifest, not a substring of that join, binds the stored text.
fn snapshot_manifest_binds_user_text(
    tx: &Transaction<'_>,
    snapshot: &ContextProjectionSnapshot,
    source: &TaskLineageSource,
    text: &str,
) -> StoreResult<bool> {
    if !snapshot.conversation_items.iter().any(|item| {
        let LogicalConversationItem::User { source_event_id, content, content_hash } = item else {
            return false;
        };
        *source_event_id == source.user_event_id
            && !content.is_empty()
            && *content_hash == emitted_text_hash(content)
    }) {
        return Ok(false);
    }
    let Ok(bytes) = artifact::read(
        tx,
        snapshot.model_egress_manifest_artifact_id,
        &snapshot.model_step_id.to_string(),
    ) else {
        return Ok(false);
    };
    let Ok(manifest) = serde_json::from_slice::<ModelEgressManifestV1>(&bytes) else {
        return Ok(false);
    };
    if manifest.manifest_hash != snapshot.model_egress_manifest_hash
        || manifest.context_snapshot_id != snapshot.id
        || manifest.validate_contract_shape().is_err()
    {
        return Ok(false);
    }
    let text_hash = emitted_text_hash(text);
    let byte_length = u64::try_from(text.len()).map_err(|_| Error::Conflict)?;
    let bound = manifest.items.iter().any(|item| {
        item.source_kind == ModelEgressSourceKind::UserEvent
            && item.source_ref == ContextSourceRef::Event { event_id: source.user_event_id }
            && matches!(item.authorization_basis, ModelEgressAuthorizationBasis::UserSubmission)
            && item.content_hash == text_hash
            && item.byte_length == byte_length
    });
    Ok(bound)
}

/// Loads only the hops required to prove `event_ids`.
///
/// An empty demand returns before any ancestor is read. Each source stops at
/// the first hop that covers it. A source that still needs more than the walk
/// budget is [`Error::Invalid`], not an ordinary uncovered source.
pub fn coverage_hops_for_events(
    db: &Connection,
    start: &crate::context::compaction::Compaction,
    event_ids: &[aura_domain_common::EventId],
) -> StoreResult<Vec<CoverageHop>> {
    if event_ids.is_empty() {
        return Ok(Vec::new());
    }
    let start_hop = hop_from(start);
    let mut merged = Vec::new();
    let mut seen = BTreeSet::new();
    for event_id in event_ids {
        let Some(text) = session::user_text(db, start.session_id, *event_id)? else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let walk = walk_coverage(
            start_hop.clone(),
            |artifact_id| load_artifact_hop(db, artifact_id),
            *event_id,
            start.session_id,
            SourceIdentity::StoredText(&text),
            COVERAGE_WALK_BUDGET,
        )?;
        match walk.verdict {
            CoverageVerdict::Covered => {
                for hop in walk.hops {
                    if seen.insert(hop.id) {
                        merged.push(hop);
                    }
                }
            }
            CoverageVerdict::NotCovered => {}
            CoverageVerdict::BudgetExhausted => return Err(Error::Invalid(COVERAGE_BUDGET_ERROR)),
        }
    }
    Ok(merged)
}

fn hop_from(compaction: &crate::context::compaction::Compaction) -> CoverageHop {
    CoverageHop::from_record(
        compaction.id,
        compaction.session_id,
        compaction.state == "completed",
        &compaction.anchors,
        compaction.excluded_event_ids.clone(),
        compaction.result_artifact_id,
        compaction.result.clone(),
    )
}

fn load_artifact_hop(
    db: &Connection,
    artifact_id: aura_domain_common::ArtifactId,
) -> StoreResult<Option<CoverageHop>> {
    let Some(previous) = crate::context::compaction::by_result_artifact(db, artifact_id)? else {
        return Ok(None);
    };
    Ok(Some(hop_from(&previous)))
}
