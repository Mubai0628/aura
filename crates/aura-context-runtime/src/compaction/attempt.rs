//! The forms a compaction request may take, and what each must satisfy before it is sent.
use std::collections::BTreeSet;

use aura_domain_common::{ContextProjectionSnapshotId, ModelStepId};
use aura_model_domain::ModelOutputFormat;

use super::{
    CompactionExecutionFailure, ContextCompactionOperationSpec, PreparedCompactionModelAttempt,
};

/// P06 §30.2 sent a
/// compaction with `tools=[]` and only its sources (`Isolated`). `SessionPrefix` is the request the
/// step would have sent — its instructions, tools, attachments and conversation unchanged, so the
/// provider's cache of them is used again — with the compaction asked after it as its closing
/// question, which no tool may answer. A tool call in the answer fails the attempt like any
/// malformed answer; nothing it asks for runs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompactionRequestShape {
    Isolated,
    SessionPrefix,
}

pub(super) fn validate_prepared_compaction_attempt(
    operation: &ContextCompactionOperationSpec,
    attempt: &PreparedCompactionModelAttempt,
    expected_ordinal: u8,
    context_snapshot_ids: &mut BTreeSet<ContextProjectionSnapshotId>,
    model_step_ids: &mut BTreeSet<ModelStepId>,
) -> Result<(), CompactionExecutionFailure> {
    let logical = &attempt.logical_request;
    let execution = &attempt.execution_snapshot;
    let isolated = attempt.shape == CompactionRequestShape::Isolated;
    let invalid = attempt.attempt_ordinal != expected_ordinal
        || logical.validate_contract_shape().is_err()
        || (isolated
            && (!logical.tool_schema_set.tools.is_empty() || !logical.attachments.is_empty()))
        || isolated == logical.closing_question.is_some()
        || logical.provider_continuation.is_some()
        || !matches!(logical.output_request.format, ModelOutputFormat::Text)
        || logical.model_step_id != execution.model_step_id
        || logical.execution_snapshot_id != execution.id
        || logical.context_snapshot_id != execution.context_snapshot_id
        || execution.model_profile_id != operation.model_profile_id
        || execution.context_snapshot_id == operation.source_context_snapshot_id
        || attempt.egress_manifest.context_snapshot_id != execution.context_snapshot_id
        || !context_snapshot_ids.insert(execution.context_snapshot_id)
        || !model_step_ids.insert(execution.model_step_id);
    if invalid {
        return Err(CompactionExecutionFailure {
            code: "context.compaction.attempt_invalid".to_owned(),
            retryable: false,
            side_effect_unknown: false,
        });
    }
    Ok(())
}
