//! Common validation of live and recovered background receipts.
use aura_domain_common::ModelStepId;
use aura_model_domain::{NormalizedModelOutput, NormalizedStopReason};
use std::{borrow::Cow, fmt::Write as _};

/// The printable form used by both the context envelope and the model request.
/// The source itself stays untouched for display, verification and egress authorization.
pub fn printable_context_text(value: &str) -> Cow<'_, str> {
    let value = value.trim_start_matches('\u{feff}');
    let needs_escape = |character: char| {
        (character.is_control() || character == '\u{feff}') && !matches!(character, '\n' | '\t')
    };
    if !value.chars().any(needs_escape) {
        return Cow::Borrowed(value);
    }
    let mut text = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\r' {
            chars.next_if_eq(&'\n');
            text.push('\n');
        } else if needs_escape(character) {
            write!(&mut text, "\\u{:04x}", u32::from(character)).expect("writing to String");
        } else {
            text.push(character);
        }
    }
    Cow::Owned(text)
}

const OPEN: &str = "<summary>";
const CLOSE: &str = "</summary>";
/// Words a first line uses that only announces the summary ("Here is the summary:").
const ANNOUNCING: [&str; 3] = ["summary", "摘要", "总结"];

/// Whether the answer holds a summary between both its tags.
pub fn has_complete_summary(output: &str) -> bool {
    output.rfind(CLOSE).is_some_and(|end| output[..end].contains(OPEN))
}

/// What the last pair of tags holds, or after the last opening tag when it was never closed;
/// without tags, the answer less a first line that only announces the summary.
pub fn background_text(output: &str) -> &str {
    if let Some(end) = output.rfind(CLOSE)
        && let Some(start) = output[..end].rfind(OPEN)
    {
        return &output[start + OPEN.len()..end];
    }
    if let Some(start) = output.rfind(OPEN) {
        return &output[start + OPEN.len()..];
    }
    let answer = output.trim_start();
    match answer.split_once('\n') {
        Some((first, rest))
            if !rest.trim().is_empty()
                && first.trim_end().ends_with([':', '：'])
                && ANNOUNCING.iter().any(|word| first.to_lowercase().contains(word)) =>
        {
            rest
        }
        _ => answer,
    }
}

/// Only a complete response from the bound summary step may replace history.
pub fn summary_answer(step: ModelStepId, output: &NormalizedModelOutput) -> Result<String, String> {
    if output.model_step_id != step {
        return Err("step_mismatch".into());
    }
    if output.refusal.is_some() {
        return Err("refusal".into());
    }
    let text = output.assistant_blocks.iter().map(|b| b.text.as_str()).collect::<String>();
    if !output.tool_calls.is_empty() && !has_complete_summary(&text) {
        return Err("tool_calls".into());
    }
    if !matches!(
        output.stop_reason,
        NormalizedStopReason::EndTurn | NormalizedStopReason::StopSequence
    ) && !(matches!(
        output.stop_reason,
        NormalizedStopReason::ToolCalls | NormalizedStopReason::MaxOutputTokens
    ) && has_complete_summary(&text))
    {
        let stop = serde_json::to_value(output.stop_reason)
            .map_err(|_| "stop_reason.unknown".to_owned())?;
        return Err(format!("stop_reason.{}", stop.as_str().unwrap_or("unknown")));
    }
    if text.contains(OPEN) && !has_complete_summary(&text) {
        return Err("incomplete_summary".into());
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let background = printable_context_text(background_text(&normalized).trim());
    if !background.chars().any(|c| c.is_alphanumeric()) {
        return Err("no_text".into());
    }
    Ok(background.into_owned())
}
