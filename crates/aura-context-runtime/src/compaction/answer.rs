//! Whether Core can use what the model answered to a compaction attempt, and why not.
use aura_model_domain::{NormalizedModelOutput, NormalizedStopReason};

use super::{
    CompactionExecutionFailure, ParsedCompactionOutput, PreparedCompactionModelAttempt,
    paragraph::{has_complete_summary, parse_paragraph_output},
};

/// How an attempt failed, for the factory to decide what the next one asks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailedCompactionAttempt {
    /// The model answered, and Core cannot use the answer; `why` is what kind of answer it was.
    UnusableAnswer { why: UnusableAnswer },
    /// No answer arrived; `retryable` as the transport judged the failure.
    NoAnswer { retryable: bool },
}

/// What kind of answer Core could not use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnusableAnswer {
    /// It called tools and gave no summary between its tags.
    ToolCalls,
    /// A refusal, another step's answer, an end before the summary was written, no text, or a
    /// summary Core cannot use.
    Other,
}

/// The summary paragraph the answer carries. A failure's code names what made the answer unusable:
/// `context.compaction.model_output_invalid.` and then `tool_calls`, `refusal`, `stop_reason.` and
/// the reason, `no_text` or `step_mismatch`, or the paragraph's own code. A summary between its
/// tags is used whatever else the answer did - a call made after it, or an end cut off once it
/// was written.
pub(super) fn usable_answer(
    attempt: &PreparedCompactionModelAttempt,
    output: Box<NormalizedModelOutput>,
    output_character_limit: u64,
) -> Result<ParsedCompactionOutput, (CompactionExecutionFailure, UnusableAnswer)> {
    let text = answered_text(attempt, *output)?;
    parse_paragraph_output(&text, output_character_limit).map_err(|error| {
        let failure = CompactionExecutionFailure {
            code: error.to_string(),
            retryable: true,
            side_effect_unknown: false,
        };
        (failure, UnusableAnswer::Other)
    })
}

fn answered_text(
    attempt: &PreparedCompactionModelAttempt,
    output: NormalizedModelOutput,
) -> Result<String, (CompactionExecutionFailure, UnusableAnswer)> {
    let text = output.assistant_blocks.iter().map(|block| block.text.as_str()).collect::<String>();
    let reason = if output.model_step_id != attempt.logical_request.model_step_id {
        Some(("step_mismatch".to_owned(), UnusableAnswer::Other))
    } else if output.refusal.is_some() {
        Some(("refusal".to_owned(), UnusableAnswer::Other))
    } else if has_complete_summary(&text) {
        None
    } else if !output.tool_calls.is_empty() {
        Some(("tool_calls".to_owned(), UnusableAnswer::ToolCalls))
    } else if !matches!(
        output.stop_reason,
        NormalizedStopReason::EndTurn | NormalizedStopReason::StopSequence
    ) {
        let name = serde_json::to_value(output.stop_reason)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown".to_owned());
        Some((format!("stop_reason.{name}"), UnusableAnswer::Other))
    } else if output.assistant_blocks.is_empty() {
        Some(("no_text".to_owned(), UnusableAnswer::Other))
    } else {
        None
    };
    if let Some((reason, why)) = reason {
        let failure = CompactionExecutionFailure {
            code: format!("context.compaction.model_output_invalid.{reason}"),
            retryable: true,
            side_effect_unknown: false,
        };
        return Err((failure, why));
    }
    Ok(text)
}
