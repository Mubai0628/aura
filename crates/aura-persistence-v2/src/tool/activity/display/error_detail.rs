use super::super::user_text;
use super::{cancelled_detail, clipped};
use aura_tool_contract::{ToolResultOutcomeV1, ToolResultV1, ToolSchemaErrorCode};
use serde_json::Value;

/// The bounded diagnostic lines a reader may expand for a terminal result of tool `name`.
///
/// The short line in `summary` is what the row shows; this is where the stored
/// explanation goes, so a truncated template is no longer the only thing the session
/// has. The first line says in Chinese what happened, what it changed and what comes next; the
/// code and the message written for the model follow as debugging lines, and each line is
/// bounded, because a Tool error is consumed by a person reading a row, not by a log viewer.
pub(super) fn error_detail(name: &str, result: &ToolResultV1) -> Vec<String> {
    let error = match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(error),
        ToolResultOutcomeV1::NotExecuted { error, .. } => error.as_ref(),
        ToolResultOutcomeV1::Cancelled { reason } => {
            return vec![cancelled_detail(*reason, result)];
        }
        ToolResultOutcomeV1::Succeeded => None,
    };
    let Some(error) = error else {
        return vec![];
    };
    let mut details: Vec<String> = user_text::line(name, result).into_iter().collect();
    if matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. })
        && error.code == ToolSchemaErrorCode::ArgumentsInvalid.error_code()
        && let Some(hints) = error.details.get("argument_hints").and_then(Value::as_array)
    {
        details
            .extend(hints.iter().filter_map(Value::as_str).take(4).map(|hint| clipped(hint, 386)));
    }
    details.push(format!("{}{}", user_text::CODE_LINE, error.code));
    let message = error.message.trim();
    if !message.is_empty() {
        details.push(format!("{}{}", user_text::MODEL_LINE, clipped(message, 400)));
    }
    if let Some(fields) = error.details.get("fields").and_then(Value::as_array) {
        let names = fields
            .iter()
            .filter_map(Value::as_str)
            .take(8)
            .map(|name| clipped(name, 64))
            .collect::<Vec<_>>();
        if !names.is_empty() {
            details.push(format!("相关字段：{}", names.join(", ")));
        }
    }
    details
}
