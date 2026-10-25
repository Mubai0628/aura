//! Coverage of one stored user source after its text has left the model context.
//!
//! A proof is bound while the stored text is still known. Later hops follow
//! artifact anchors and that proof; they do not read a summary string and they
//! do not stop at a fixed depth. Exhausting the walk budget is a different
//! result from a source that is simply not covered.

use crate::{
    CompactionAnchorKind, CompactionAnchorManifest, ContextReadCompactionResult, ContextSourceRef,
    anchor_matches_user_history, compaction_result_anchor_hash, emitted_text_hash,
};
use aura_domain_common::{
    ArtifactId, ContentHash, ContextProjectionSnapshotId, EventId, SessionId,
};
use std::collections::BTreeSet;

/// Resource limit for hops actually judged while proving one source.
///
/// Ancestors behind a hop that already covers the source are not visited.
/// A source that still needs more than this many hops is budget exhaustion,
/// never an ordinary missing source.
pub const COVERAGE_WALK_BUDGET: usize = 256;
pub const COVERAGE_BUDGET_ERROR: &str = "context.compaction.coverage_budget";

/// Binding created from one stored user text and the selector anchor of that
/// same history item. It carries neither the text nor a summary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserSourceProof {
    pub event_id: EventId,
    pub content_hash: ContentHash,
    pub anchor_hash: ContentHash,
}

/// One persisted compaction hop. `manifest_matches` is the stored anchor list
/// compared with the result the next hop hashes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageHop {
    pub id: ContextProjectionSnapshotId,
    pub session_id: SessionId,
    pub completed: bool,
    pub manifest_matches: bool,
    pub excluded_event_ids: Vec<EventId>,
    pub result_artifact_id: Option<ArtifactId>,
    pub result: Option<ContextReadCompactionResult>,
}

impl CoverageHop {
    #[must_use]
    pub fn from_record(
        id: ContextProjectionSnapshotId,
        session_id: SessionId,
        completed: bool,
        anchors: &CompactionAnchorManifest,
        excluded_event_ids: Vec<EventId>,
        result_artifact_id: Option<ArtifactId>,
        result: Option<ContextReadCompactionResult>,
    ) -> Self {
        let manifest_matches =
            result.as_ref().is_some_and(|result| result.anchor_manifest == *anchors);
        Self {
            id,
            session_id,
            completed,
            manifest_matches,
            excluded_event_ids,
            result_artifact_id,
            result,
        }
    }
}

/// Binds a proof only when this anchor is the selector hash of this exact text.
#[must_use]
pub fn prove_user_source(
    event_id: EventId,
    text: &str,
    anchor_hash: &ContentHash,
) -> Option<UserSourceProof> {
    if text.is_empty() || !anchor_matches_user_history(anchor_hash, event_id, text) {
        return None;
    }
    Some(UserSourceProof {
        event_id,
        content_hash: emitted_text_hash(text),
        anchor_hash: anchor_hash.clone(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoverageVerdict {
    Covered,
    NotCovered,
    BudgetExhausted,
}

#[derive(Clone, Copy, Debug)]
pub enum SourceIdentity<'a> {
    /// The stored user text is still available, so the event anchor is checked
    /// against the selector hash directly.
    StoredText(&'a str),
    /// The text has already left the model context. The proof was bound earlier
    /// from that same text.
    Proof(&'a UserSourceProof),
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ArtifactLink {
    artifact_id: ArtifactId,
    source_hash: ContentHash,
}

/// Hops visited while proving one source, and the verdict for that source.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CoverageWalk {
    pub hops: Vec<CoverageHop>,
    pub verdict: CoverageVerdict,
}

/// Walks from `start` and loads the next artifact only when this source still
/// needs it. A hop that already covers the source ends the walk. A cycle is not
/// coverage. Hitting `budget` before the source is proved is
/// [`CoverageVerdict::BudgetExhausted`].
pub fn walk_coverage<E, F>(
    start: CoverageHop,
    mut load_artifact: F,
    event_id: EventId,
    session_id: SessionId,
    identity: SourceIdentity<'_>,
    budget: usize,
) -> Result<CoverageWalk, E>
where
    F: FnMut(ArtifactId) -> Result<Option<CoverageHop>, E>,
{
    let mut hops = Vec::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![start];
    let mut judged = 0_usize;
    while let Some(hop) = pending.pop() {
        if !seen.insert(hop.id) {
            continue;
        }
        if judged == budget {
            return Ok(CoverageWalk { hops, verdict: CoverageVerdict::BudgetExhausted });
        }
        judged += 1;
        match judge_hop(&hop, event_id, session_id, identity) {
            HopFollow::Covered => {
                hops.push(hop);
                return Ok(CoverageWalk { hops, verdict: CoverageVerdict::Covered });
            }
            HopFollow::Stop => hops.push(hop),
            HopFollow::Continue(links) => {
                for link in links {
                    let Some(next) = load_artifact(link.artifact_id)? else {
                        continue;
                    };
                    if next.result_artifact_id != Some(link.artifact_id) {
                        continue;
                    }
                    let Some(result) = next.result.as_ref() else {
                        continue;
                    };
                    let Ok(hash) = compaction_result_anchor_hash(result) else {
                        continue;
                    };
                    if hash == link.source_hash {
                        pending.push(next);
                    }
                }
                hops.push(hop);
            }
        }
    }
    Ok(CoverageWalk { hops, verdict: CoverageVerdict::NotCovered })
}

/// Walks from the latest hop through artifact anchors.
///
/// `others` holds earlier hops addressable by `result_artifact_id`. A cycle is
/// not coverage. Hitting `budget` before the walk finishes is
/// [`CoverageVerdict::BudgetExhausted`].
#[must_use]
pub fn follow_coverage(
    start: &CoverageHop,
    others: &[CoverageHop],
    event_id: EventId,
    session_id: SessionId,
    identity: SourceIdentity<'_>,
    budget: usize,
) -> CoverageVerdict {
    let mut seen = BTreeSet::new();
    let mut pending = vec![start];
    let mut judged = 0_usize;
    let mut exhausted = false;
    while let Some(hop) = pending.pop() {
        if !seen.insert(hop.id) {
            continue;
        }
        if judged == budget {
            exhausted = true;
            break;
        }
        judged += 1;
        match judge_hop(hop, event_id, session_id, identity) {
            HopFollow::Covered => return CoverageVerdict::Covered,
            HopFollow::Stop => {}
            HopFollow::Continue(links) => {
                for link in links {
                    let Some(next) = others.iter().find(|candidate| {
                        candidate.result_artifact_id == Some(link.artifact_id)
                            && candidate
                                .result
                                .as_ref()
                                .and_then(|result| compaction_result_anchor_hash(result).ok())
                                .is_some_and(|hash| hash == link.source_hash)
                    }) else {
                        continue;
                    };
                    pending.push(next);
                }
            }
        }
    }
    if exhausted { CoverageVerdict::BudgetExhausted } else { CoverageVerdict::NotCovered }
}

enum HopFollow {
    Covered,
    Continue(Vec<ArtifactLink>),
    Stop,
}

fn judge_hop(
    hop: &CoverageHop,
    event_id: EventId,
    session_id: SessionId,
    identity: SourceIdentity<'_>,
) -> HopFollow {
    let Some(result) = hop.result.as_ref() else {
        return HopFollow::Stop;
    };
    if !hop.completed
        || !hop.manifest_matches
        || hop.session_id != session_id
        || !hop.excluded_event_ids.contains(&event_id)
    {
        return HopFollow::Stop;
    }
    let spanned = result
        .source_span
        .source_refs
        .iter()
        .any(|source| source == &ContextSourceRef::Event { event_id });
    let anchored = result.anchor_manifest.anchors.iter().any(|anchor| {
        anchor.kind == CompactionAnchorKind::Event
            && anchor.source_ref == ContextSourceRef::Event { event_id }
            && spanned
            && identity_matches(identity, event_id, &anchor.source_hash)
    });
    if anchored {
        return HopFollow::Covered;
    }
    let links = result
        .anchor_manifest
        .anchors
        .iter()
        .filter_map(|anchor| {
            if anchor.kind != CompactionAnchorKind::Artifact {
                return None;
            }
            let ContextSourceRef::Artifact { artifact_id } = anchor.source_ref else {
                return None;
            };
            Some(ArtifactLink { artifact_id, source_hash: anchor.source_hash.clone() })
        })
        .collect::<Vec<_>>();
    if links.is_empty() { HopFollow::Stop } else { HopFollow::Continue(links) }
}

fn identity_matches(
    identity: SourceIdentity<'_>,
    event_id: EventId,
    anchor_hash: &ContentHash,
) -> bool {
    match identity {
        SourceIdentity::StoredText(text) => {
            anchor_matches_user_history(anchor_hash, event_id, text)
        }
        SourceIdentity::Proof(proof) => {
            proof.event_id == event_id && proof.anchor_hash == *anchor_hash
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CompactionAnchor, CompactionSourceSpan};
    use aura_domain_common::SessionSequence;

    fn event_hop(
        session_id: SessionId,
        event_id: EventId,
        text: &str,
    ) -> (CoverageHop, UserSourceProof) {
        let anchor_hash = crate::user_history_anchor_hash(event_id, text).expect("anchor");
        let proof = prove_user_source(event_id, text, &anchor_hash).expect("proof");
        let anchors = vec![CompactionAnchor {
            anchor: "event".into(),
            kind: CompactionAnchorKind::Event,
            source_ref: ContextSourceRef::Event { event_id },
            source_hash: anchor_hash,
            preview: "preview".into(),
        }];
        let manifest =
            CompactionAnchorManifest { manifest_hash: ContentHash::sha256(b"event"), anchors };
        let result = ContextReadCompactionResult {
            source_context_snapshot_id: ContextProjectionSnapshotId::new(),
            sequence: 1,
            historical_snapshot: "history".into(),
            active_turn_summary: "active".into(),
            structured_compaction: "structured".into(),
            source_span: CompactionSourceSpan {
                start_sequence: SessionSequence::new(1),
                end_sequence: SessionSequence::new(1),
                source_refs: vec![ContextSourceRef::Event { event_id }],
            },
            anchor_manifest: manifest.clone(),
        };
        let hop = CoverageHop::from_record(
            result.source_context_snapshot_id,
            session_id,
            true,
            &manifest,
            vec![event_id],
            Some(ArtifactId::new()),
            Some(result),
        );
        (hop, proof)
    }

    fn artifact_hop(
        session_id: SessionId,
        event_id: EventId,
        previous: &CoverageHop,
        source_hash: ContentHash,
    ) -> CoverageHop {
        let artifact_id = previous.result_artifact_id.expect("artifact");
        let anchors = vec![CompactionAnchor {
            anchor: "previous".into(),
            kind: CompactionAnchorKind::Artifact,
            source_ref: ContextSourceRef::Artifact { artifact_id },
            source_hash,
            preview: "previous".into(),
        }];
        let manifest =
            CompactionAnchorManifest { manifest_hash: ContentHash::sha256(b"artifact"), anchors };
        let id = ContextProjectionSnapshotId::new();
        let result = ContextReadCompactionResult {
            source_context_snapshot_id: id,
            sequence: 2,
            historical_snapshot: "later".into(),
            active_turn_summary: "later".into(),
            structured_compaction: "later".into(),
            source_span: CompactionSourceSpan {
                start_sequence: SessionSequence::new(2),
                end_sequence: SessionSequence::new(2),
                source_refs: vec![ContextSourceRef::Artifact { artifact_id }],
            },
            anchor_manifest: manifest.clone(),
        };
        CoverageHop::from_record(
            id,
            session_id,
            true,
            &manifest,
            vec![event_id],
            Some(ArtifactId::new()),
            Some(result),
        )
    }

    #[test]
    fn a_body_hash_does_not_bind_a_source_proof() {
        let text = "Compacted addition.";
        let event_id = EventId::new();
        assert!(prove_user_source(event_id, text, &ContentHash::sha256(text.as_bytes())).is_none());
        assert!(prove_user_source(event_id, text, &ContentHash::blake3(text.as_bytes())).is_none());
    }

    #[test]
    fn nine_hops_stay_covered_and_a_short_budget_is_not_a_missing_source() {
        let session_id = SessionId::new();
        let event_id = EventId::new();
        let text = "Compacted consumed addition";
        let (mut previous, proof) = event_hop(session_id, event_id, text);
        let mut hops = vec![previous.clone()];
        for _ in 1..9 {
            let hash = compaction_result_anchor_hash(previous.result.as_ref().expect("result"))
                .expect("hash");
            previous = artifact_hop(session_id, event_id, &previous, hash);
            hops.push(previous.clone());
        }
        let latest = hops.last().expect("latest").clone();
        let verdict = follow_coverage(
            &latest,
            &hops,
            event_id,
            session_id,
            SourceIdentity::Proof(&proof),
            COVERAGE_WALK_BUDGET,
        );
        assert_eq!(verdict, CoverageVerdict::Covered);
        assert_eq!(
            follow_coverage(
                &latest,
                &hops,
                event_id,
                session_id,
                SourceIdentity::StoredText(text),
                8
            ),
            CoverageVerdict::BudgetExhausted
        );
        assert_eq!(
            follow_coverage(
                &latest,
                &hops,
                event_id,
                session_id,
                SourceIdentity::StoredText(text),
                9
            ),
            CoverageVerdict::Covered
        );
    }

    #[test]
    fn a_broken_hash_session_or_cycle_is_not_coverage() {
        let session_id = SessionId::new();
        let event_id = EventId::new();
        let text = "Compacted consumed addition";
        let (origin, proof) = event_hop(session_id, event_id, text);
        let broken =
            artifact_hop(session_id, event_id, &origin, ContentHash::sha256(b"not-the-result"));
        assert_eq!(
            follow_coverage(
                &broken,
                std::slice::from_ref(&origin),
                event_id,
                session_id,
                SourceIdentity::Proof(&proof),
                COVERAGE_WALK_BUDGET,
            ),
            CoverageVerdict::NotCovered
        );
        let foreign = {
            let mut hop = origin.clone();
            hop.session_id = SessionId::new();
            hop
        };
        assert_eq!(
            follow_coverage(
                &foreign,
                &[],
                event_id,
                session_id,
                SourceIdentity::StoredText(text),
                COVERAGE_WALK_BUDGET,
            ),
            CoverageVerdict::NotCovered
        );
        let left_artifact = ArtifactId::new();
        let right_artifact = ArtifactId::new();
        let mut left = artifact_hop(
            session_id,
            event_id,
            &origin,
            compaction_result_anchor_hash(origin.result.as_ref().expect("origin")).expect("hash"),
        );
        left.result_artifact_id = Some(left_artifact);
        let mut right = artifact_hop(
            session_id,
            event_id,
            &left,
            compaction_result_anchor_hash(left.result.as_ref().expect("left")).expect("hash"),
        );
        right.result_artifact_id = Some(right_artifact);
        // Replace the link on the earlier hop so the two artifact hops point only
        // at each other. Neither still has an event anchor.
        if let Some(result) = left.result.as_mut() {
            result.anchor_manifest.anchors[0].source_ref =
                ContextSourceRef::Artifact { artifact_id: right_artifact };
            result.anchor_manifest.anchors[0].source_hash =
                compaction_result_anchor_hash(right.result.as_ref().expect("right")).expect("hash");
        }
        assert_eq!(
            follow_coverage(
                &right,
                &[left],
                event_id,
                session_id,
                SourceIdentity::StoredText(text),
                COVERAGE_WALK_BUDGET,
            ),
            CoverageVerdict::NotCovered
        );
        assert_eq!(
            follow_coverage(
                &broken,
                &[],
                event_id,
                session_id,
                SourceIdentity::Proof(&proof),
                COVERAGE_WALK_BUDGET,
            ),
            CoverageVerdict::NotCovered,
            "a missing previous artifact is a broken chain"
        );
    }

    #[test]
    fn a_covering_hop_does_not_load_history_behind_it() {
        let session_id = SessionId::new();
        let event_id = EventId::new();
        let text = "Compacted consumed addition";
        let (mut hop, proof) = event_hop(session_id, event_id, text);
        let unrelated = ArtifactId::new();
        let result = hop.result.as_mut().expect("result");
        result.anchor_manifest.anchors.push(CompactionAnchor {
            anchor: "previous".into(),
            kind: CompactionAnchorKind::Artifact,
            source_ref: ContextSourceRef::Artifact { artifact_id: unrelated },
            source_hash: ContentHash::sha256(b"unrelated-history"),
            preview: "previous".into(),
        });
        let mut loads = 0_usize;
        let walk = walk_coverage(
            hop,
            |_| {
                loads += 1;
                Ok::<_, ()>(None)
            },
            event_id,
            session_id,
            SourceIdentity::Proof(&proof),
            1,
        )
        .expect("walk");
        assert_eq!(walk.verdict, CoverageVerdict::Covered);
        assert_eq!(loads, 0);
        assert_eq!(walk.hops.len(), 1);
    }

    #[test]
    fn a_relevant_chain_past_the_budget_is_exhausted() {
        let session_id = SessionId::new();
        let event_id = EventId::new();
        let text = "Compacted consumed addition";
        let (mut previous, proof) = event_hop(session_id, event_id, text);
        let mut by_artifact = std::collections::BTreeMap::new();
        by_artifact.insert(previous.result_artifact_id.expect("artifact"), previous.clone());
        for _ in 1..257 {
            let hash = compaction_result_anchor_hash(previous.result.as_ref().expect("result"))
                .expect("hash");
            previous = artifact_hop(session_id, event_id, &previous, hash);
            by_artifact.insert(previous.result_artifact_id.expect("artifact"), previous.clone());
        }
        let exhausted = walk_coverage(
            previous.clone(),
            |artifact_id| Ok::<_, ()>(by_artifact.get(&artifact_id).cloned()),
            event_id,
            session_id,
            SourceIdentity::Proof(&proof),
            COVERAGE_WALK_BUDGET,
        )
        .expect("walk");
        assert_eq!(exhausted.verdict, CoverageVerdict::BudgetExhausted);
        let covered = walk_coverage(
            previous,
            |artifact_id| Ok::<_, ()>(by_artifact.get(&artifact_id).cloned()),
            event_id,
            session_id,
            SourceIdentity::StoredText(text),
            COVERAGE_WALK_BUDGET + 1,
        )
        .expect("walk");
        assert_eq!(covered.verdict, CoverageVerdict::Covered);
    }
}
