//! A compacted addition stays covered after its text leaves the model context.
//!
//! The proof is bound from the stored text and the selector anchor. A summary
//! string is not a proof. Walking a long chain uses the same verdict as the
//! Store, including a budget that is distinct from a missing source.

use crate::preparation::AcceptedTurnAdditionV1;
use crate::section::{ContextFragment, ContextSectionAssembly};
use aura_context_domain::{
    COVERAGE_WALK_BUDGET, ContextSourceRef, CoverageHop, CoverageVerdict, SourceIdentity,
    UserSourceProof, follow_coverage,
};

pub(crate) enum AdditionCoverage {
    Covered,
    Missing,
    BudgetExhausted,
}

pub(crate) fn compaction_covers(
    assembly: &ContextSectionAssembly,
    addition: &AcceptedTurnAdditionV1,
    proofs: &[UserSourceProof],
    hops: &[CoverageHop],
) -> AdditionCoverage {
    let text = user_submission_text(assembly, addition);
    let identity = if let Some(text) = text {
        SourceIdentity::StoredText(text)
    } else if let Some(proof) = proofs.iter().find(|proof| {
        proof.event_id == addition.user_event_id && proof.content_hash == addition.content_hash
    }) {
        SourceIdentity::Proof(proof)
    } else {
        return AdditionCoverage::Missing;
    };
    let mut exhausted = false;
    for start in visible_starts(assembly, hops) {
        match follow_coverage(
            &start,
            hops,
            addition.user_event_id,
            addition.session_id,
            identity,
            COVERAGE_WALK_BUDGET,
        ) {
            CoverageVerdict::Covered => return AdditionCoverage::Covered,
            CoverageVerdict::BudgetExhausted => exhausted = true,
            CoverageVerdict::NotCovered => {}
        }
    }
    if exhausted { AdditionCoverage::BudgetExhausted } else { AdditionCoverage::Missing }
}

fn user_submission_text<'a>(
    assembly: &'a ContextSectionAssembly,
    addition: &AcceptedTurnAdditionV1,
) -> Option<&'a str> {
    assembly.egress_fragments.iter().find_map(|fragment| {
        let ContextFragment { source_ref, source_kind, authorization_basis, content, .. } =
            fragment;
        (*source_ref == ContextSourceRef::Event { event_id: addition.user_event_id }
            && *source_kind == aura_context_domain::ModelEgressSourceKind::UserEvent
            && matches!(
                authorization_basis,
                aura_context_domain::ModelEgressAuthorizationBasis::UserSubmission
            )
            && aura_context_domain::emitted_text_hash(content) == addition.content_hash)
            .then_some(content.as_str())
    })
}

/// The compactions the request shows, as the store recorded what each replaced. The section
/// shows the stored summary's text, so a hop counts only when its summary is what is shown.
fn visible_starts(assembly: &ContextSectionAssembly, hops: &[CoverageHop]) -> Vec<CoverageHop> {
    assembly
        .egress_fragments
        .iter()
        .filter(|fragment| {
            matches!(fragment.source_ref, ContextSourceRef::Compaction { .. })
                && fragment.source_kind == aura_context_domain::ModelEgressSourceKind::Compaction
        })
        .filter_map(|fragment| {
            let ContextSourceRef::Compaction { context_snapshot_id } = fragment.source_ref else {
                return None;
            };
            let hop = hops.iter().find(|hop| hop.id == context_snapshot_id)?;
            let result = hop.result.as_ref()?;
            (result.source_context_snapshot_id == context_snapshot_id
                && fragment.content.contains(result.historical_snapshot.trim()))
            .then(|| hop.clone())
        })
        .collect()
}
