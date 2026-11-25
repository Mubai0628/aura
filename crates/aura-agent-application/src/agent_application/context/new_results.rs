//! Admission of tool results which have not been shown to the model yet.
use super::*;
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};

const PREVIEW_BYTES: usize = 1_024;
const MIN_REFERENCE_BYTES: usize = 2 * PREVIEW_BYTES;

#[derive(Default)]
pub(super) struct Reduction {
    pub originals: Vec<(ToolCallId, String)>,
    pub bytes_released: u64,
}

pub(super) fn shorten(
    input: &mut ContextPreparationInput,
    budget: &ContextBudgetSnapshot,
    tokens_needed: u64,
) -> anyhow::Result<Reduction> {
    let mut reduced = Reduction::default();
    let Some(facts) = &input.tool_continuation_facts else { return Ok(reduced) };
    let pending: BTreeSet<_> = facts.durable_result_tool_call_ids.iter().copied().collect();
    let adopted: BTreeSet<_> = input
        .previous_snapshot
        .iter()
        .flat_map(|snapshot| &snapshot.conversation_items)
        .filter_map(|item| match item {
            LogicalConversationItem::ToolResult { tool_call_id, .. } => Some(*tool_call_id),
            _ => None,
        })
        .collect();
    let estimates: BTreeMap<_, _> = budget
        .item_estimates
        .iter()
        .map(|item| (item.content_hash.hex.as_str(), item.estimated_tokens))
        .collect();

    let mut candidates = Vec::new();
    for (index, item) in input.recent_history_items.iter().enumerate() {
        if let LogicalConversationItem::ToolResult { tool_call_id, content, content_hash, .. } =
            item
            && pending.contains(tool_call_id)
            && !adopted.contains(tool_call_id)
            && content.len() > MIN_REFERENCE_BYTES
            && !is_tool_result_reference(content)
        {
            let cost = estimates
                .get(content_hash.hex.as_str())
                .copied()
                .unwrap_or_else(|| reference_text_tokens(content));
            candidates.push((Reverse(cost), index));
        }
    }
    candidates.sort_unstable();

    let mut saved = 0;
    for (Reverse(cost), index) in candidates {
        let LogicalConversationItem::ToolResult {
            tool_call_id,
            tool_name,
            content,
            content_hash,
            is_error,
            ..
        } = &mut input.recent_history_items[index]
        else {
            unreachable!("only tool results are admitted as candidates");
        };
        let projected = reference(*tool_call_id, tool_name, content, *is_error);
        let released = cost.saturating_sub(reference_text_tokens(&projected));
        if released == 0 {
            continue;
        }
        saved += released;
        reduced.bytes_released += content.len().saturating_sub(projected.len()) as u64;
        *content_hash = ContentHash::sha256(projected.as_bytes());
        reduced.originals.push((*tool_call_id, std::mem::replace(content, projected)));
        if saved >= tokens_needed {
            break;
        }
    }
    if !reduced.originals.is_empty() {
        super::clearing::rebuild(input)?;
    }
    Ok(reduced)
}

fn reference(id: ToolCallId, name: &str, body: &str, is_error: bool) -> String {
    let end = body.floor_char_boundary(PREVIEW_BYTES.min(body.len()));
    let state = if is_error { "non-success" } else { "succeeded" };
    let mut receipt = format!(
        "{TOOL_RESULT_REFERENCE_PREFIX} {name}, {state}, tool_call_id {id}. \
         Full model-readable result: {} bytes. Available with context.read_tool_result. \
         Preview: first {end} bytes.]",
        body.len()
    );
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
        let value = &json["outcome"]["payload"]["value"];
        let mut metadata = serde_json::Map::new();
        for key in [
            "exit_code",
            "timed_out",
            "truncated",
            "output_truncated",
            "stdout_total",
            "stderr_total",
            "stdout_next",
            "stderr_next",
        ] {
            if value[key].is_number() || value[key].is_boolean() {
                metadata.insert(key.into(), value[key].clone());
            }
        }
        if let Some(effect) = json["side_effect_state"].as_str() {
            metadata.insert("side_effect_state".into(), effect.into());
        }
        if let Some(outcome) = json["outcome"]["type"].as_str() {
            metadata.insert("outcome".into(), outcome.into());
        }
        if name == "fs.read" {
            for key in ["path", "logical_hash", "returned_range", "total_lines"] {
                if let Some(field) = value.get(key).filter(|field| field.to_string().len() <= 512) {
                    metadata.insert(key.into(), field.clone());
                }
            }
        }
        if !metadata.is_empty() {
            receipt.push('\n');
            receipt.push_str(&serde_json::Value::Object(metadata).to_string());
        }
    }
    receipt.push('\n');
    receipt.push_str(&body[..end]);
    receipt
}
