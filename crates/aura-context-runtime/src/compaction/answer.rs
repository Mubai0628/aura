use super::{CompactionExecutionFailure, ParsedCompactionOutput, PreparedCompactionModelAttempt};
use aura_model_domain::NormalizedModelOutput;

pub(super) fn usable_answer(
    attempt: &PreparedCompactionModelAttempt,
    output: &NormalizedModelOutput,
) -> Result<ParsedCompactionOutput, CompactionExecutionFailure> {
    let historical_snapshot =
        aura_context_domain::summary_answer(attempt.logical_request.model_step_id, output)
            .map_err(|reason| CompactionExecutionFailure {
                code: format!("context.compaction.model_output_invalid.{reason}"),
                retryable: false,
                side_effect_unknown: false,
            })?;
    Ok(ParsedCompactionOutput {
        full_markdown: historical_snapshot.clone(),
        historical_snapshot,
        active_turn_summary: String::new(),
        structured_compaction: String::new(),
        referenced_anchors: vec![],
    })
}
