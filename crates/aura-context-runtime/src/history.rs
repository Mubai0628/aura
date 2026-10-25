use std::collections::{BTreeMap, BTreeSet};

use aura_context_domain::{
    ContextRetentionException, HistoryAtomicGroupRefV1, LARGE_TOOL_RESULT_PREFIX_CHARS,
    LARGE_TOOL_RESULT_SUFFIX_CHARS, LARGE_TOOL_RESULT_THRESHOLD_CHARS,
    ToolResultContextProjectionV1,
};
use aura_domain_common::ToolCallId;
use aura_domain_common::{ArtifactId, ContentHash, EventId, SessionSequence};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HistoryEventKind {
    UserMessage,
    AssistantMessage,
    ToolCall { tool_call_id: ToolCallId },
    ToolResult { tool_call_id: ToolCallId },
    Approval,
    Query,
    QueryAnswer,
    Recovery,
    State,
}

#[derive(Clone, Debug)]
pub struct HistoryEventUnit {
    pub event_id: EventId,
    pub sequence: SessionSequence,
    pub turn_segment_id: String,
    pub history_atomic_group_ref: Option<HistoryAtomicGroupRefV1>,
    pub atomic_group_id: Option<String>,
    pub kind: HistoryEventKind,
    pub estimated_tokens: u64,
    pub sealed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecentHistoryPlan {
    pub compact_event_ids: Vec<EventId>,
    pub retained_event_ids: Vec<EventId>,
    pub compacted_tokens: u64,
    pub retained_recent_tokens: u64,
    pub retention_exceptions: Vec<ContextRetentionException>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum HistoryPlannerError {
    #[error("context.history.sequence_order_invalid")]
    SequenceOrder,
    #[error("context.history.atomic_group_not_contiguous")]
    AtomicGroupNotContiguous,
    #[error("context.history.tool_pair_incomplete")]
    ToolPairIncomplete,
    #[error("context.history.arithmetic_overflow")]
    ArithmeticOverflow,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolResultContextProjectionError {
    #[error("context.tool_result.full_length_mismatch")]
    FullLengthMismatch,
    #[error("context.tool_result.full_content_hash_mismatch")]
    FullContentHashMismatch,
}

pub fn plan_recent_history(
    runtime_effective_window_tokens: u64,
    request_input_budget_tokens: u64,
    current_non_history_tokens: u64,
    events: &[HistoryEventUnit],
) -> Result<RecentHistoryPlan, HistoryPlannerError> {
    validate_history_units(events)?;
    // Replaced P06
    // §27.3's 16%–30%. 8%–10%, so a compaction leaves the request at about 12%–15% of the window with its fixed
    // content and summary.
    let min_recent = percentage_ceiling(runtime_effective_window_tokens, 8)?;
    let boundary_limit = percentage_ceiling(runtime_effective_window_tokens, 10)?;
    let groups = atomic_units(events)?;
    // What stays after a compaction leaves a quarter of the budget for the next steps, so
    // one compaction is not followed by another at the next step. The parts no compaction
    // removes — the fixed content and the unsealed current groups — come first.
    let unsealed = groups
        .iter()
        .filter(|group| !group.sealed)
        .try_fold(0_u64, |total, group| total.checked_add(group.estimated_tokens))
        .ok_or(HistoryPlannerError::ArithmeticOverflow)?;
    let available = (request_input_budget_tokens / 4 * 3)
        .saturating_sub(current_non_history_tokens)
        .saturating_sub(unsealed);
    let retention_cap = available.min(boundary_limit.max(min_recent));
    let mut retained_groups = BTreeSet::new();
    let mut retained_tokens = 0_u64;
    let mut exceptions = Vec::new();
    for (index, group) in groups.iter().enumerate().rev() {
        if !group.sealed {
            continue;
        }
        let Some(next) = retained_tokens.checked_add(group.estimated_tokens) else {
            return Err(HistoryPlannerError::ArithmeticOverflow);
        };
        // Two percent is narrower than one step's group, so the group that passes the
        // cap before the band's floor is reached stays when that leaves what is kept nearer the
        // band than stopping short would; the real DeepSeek run otherwise kept 7% of its window.
        let nearer = retained_tokens < min_recent
            && next <= available
            && next.saturating_sub(retention_cap) <= min_recent - retained_tokens;
        if next > retention_cap && !nearer {
            if retained_tokens < min_recent {
                exceptions.push(ContextRetentionException::OversizedAtomicSegment);
            }
            continue;
        }
        if retained_tokens < min_recent || next <= boundary_limit {
            retained_groups.insert(index);
            retained_tokens = next;
        }
    }
    if retained_tokens < min_recent {
        exceptions.push(ContextRetentionException::InsufficientEligibleRecentHistory);
    }
    exceptions.sort_unstable_by_key(|value| match value {
        ContextRetentionException::OversizedAtomicSegment => 0,
        ContextRetentionException::InsufficientEligibleRecentHistory => 1,
    });
    exceptions.dedup();

    let mut compact_event_ids = Vec::new();
    let mut retained_event_ids = Vec::new();
    let mut compacted_tokens = 0_u64;
    for (index, group) in groups.iter().enumerate() {
        if retained_groups.contains(&index) || !group.sealed {
            retained_event_ids.extend(group.event_ids.iter().copied());
        } else {
            compact_event_ids.extend(group.event_ids.iter().copied());
            compacted_tokens = compacted_tokens
                .checked_add(group.estimated_tokens)
                .ok_or(HistoryPlannerError::ArithmeticOverflow)?;
        }
    }
    Ok(RecentHistoryPlan {
        compact_event_ids,
        retained_event_ids,
        compacted_tokens,
        retained_recent_tokens: retained_tokens,
        retention_exceptions: exceptions,
    })
}

/// When what [`plan_recent_history`] keeps still does not fit, every sealed group is
/// compacted; only the unsealed current groups stay as they are.
pub fn plan_compact_all_sealed(
    events: &[HistoryEventUnit],
) -> Result<RecentHistoryPlan, HistoryPlannerError> {
    validate_history_units(events)?;
    let mut plan = RecentHistoryPlan {
        compact_event_ids: Vec::new(),
        retained_event_ids: Vec::new(),
        compacted_tokens: 0,
        retained_recent_tokens: 0,
        retention_exceptions: Vec::new(),
    };
    for group in atomic_units(events)? {
        if group.sealed {
            plan.compact_event_ids.extend(group.event_ids);
            plan.compacted_tokens = plan
                .compacted_tokens
                .checked_add(group.estimated_tokens)
                .ok_or(HistoryPlannerError::ArithmeticOverflow)?;
        } else {
            plan.retained_event_ids.extend(group.event_ids);
        }
    }
    Ok(plan)
}

#[derive(Clone, Debug)]
struct AtomicUnit {
    event_ids: Vec<EventId>,
    estimated_tokens: u64,
    sealed: bool,
}

fn atomic_units(events: &[HistoryEventUnit]) -> Result<Vec<AtomicUnit>, HistoryPlannerError> {
    let mut groups = Vec::new();
    let mut index = 0;
    while index < events.len() {
        let event = &events[index];
        let key = history_atomic_group_key(event);
        let mut event_ids = Vec::new();
        let mut estimated_tokens = 0_u64;
        let mut sealed = true;
        while index < events.len() && history_atomic_group_key(&events[index]) == key {
            event_ids.push(events[index].event_id);
            estimated_tokens = estimated_tokens
                .checked_add(events[index].estimated_tokens)
                .ok_or(HistoryPlannerError::ArithmeticOverflow)?;
            sealed &= events[index].sealed;
            index += 1;
        }
        groups.push(AtomicUnit { event_ids, estimated_tokens, sealed });
    }
    Ok(groups)
}

fn validate_history_units(events: &[HistoryEventUnit]) -> Result<(), HistoryPlannerError> {
    if !events.windows(2).all(|pair| pair[0].sequence < pair[1].sequence) {
        return Err(HistoryPlannerError::SequenceOrder);
    }
    let mut closed_groups = BTreeSet::new();
    let mut current_group = None::<String>;
    for event in events {
        let next_group = history_atomic_group_key(event);
        if current_group.as_ref() != Some(&next_group) {
            if let Some(group) = current_group.take() {
                closed_groups.insert(group);
            }
            if closed_groups.contains(&next_group) {
                return Err(HistoryPlannerError::AtomicGroupNotContiguous);
            }
            current_group = Some(next_group);
        }
    }
    let mut calls = BTreeMap::<ToolCallId, (usize, usize)>::new();
    for event in events {
        match event.kind {
            HistoryEventKind::ToolCall { tool_call_id } => {
                calls.entry(tool_call_id).or_default().0 += 1
            }
            HistoryEventKind::ToolResult { tool_call_id } => {
                calls.entry(tool_call_id).or_default().1 += 1;
            }
            _ => {}
        }
    }
    if calls.values().any(|counts| *counts != (1, 1)) {
        return Err(HistoryPlannerError::ToolPairIncomplete);
    }
    Ok(())
}

fn history_atomic_group_key(event: &HistoryEventUnit) -> String {
    if let Some(group_ref) = &event.history_atomic_group_ref {
        return format!(
            "v1:{}:{}",
            group_ref.assistant_response_event_id,
            group_ref.group_hash.hex()
        );
    }
    event
        .atomic_group_id
        .as_ref()
        .map(|group| format!("v0:{group}"))
        .unwrap_or_else(|| format!("event:{}", event.event_id))
}

fn percentage_ceiling(value: u64, percent: u64) -> Result<u64, HistoryPlannerError> {
    value
        .checked_mul(percent)
        .and_then(|value| value.checked_add(99))
        .map(|value| value / 100)
        .ok_or(HistoryPlannerError::ArithmeticOverflow)
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LargeToolResultPreview {
    pub text: String,
    pub preview_is_complete: bool,
}

#[must_use]
pub fn large_tool_result_preview(text: &str, artifact_reference: &str) -> LargeToolResultPreview {
    let characters = text.chars().count();
    if characters <= LARGE_TOOL_RESULT_THRESHOLD_CHARS {
        return LargeToolResultPreview { text: text.to_owned(), preview_is_complete: true };
    }
    let prefix = text.chars().take(LARGE_TOOL_RESULT_PREFIX_CHARS).collect::<String>();
    let suffix = text
        .chars()
        .skip(characters.saturating_sub(LARGE_TOOL_RESULT_SUFFIX_CHARS))
        .collect::<String>();
    LargeToolResultPreview {
        text: format!(
            "{prefix}\n[...... tool result middle pruned ......]\n{suffix}\nOriginal artifact: {artifact_reference}"
        ),
        preview_is_complete: false,
    }
}

/// Builds the only model-context view of a durable P07 model-visible Result.
///
/// The complete envelope remains in the Artifact store. P05 receives only the
/// returned preview through the P06 request assembly path and therefore never
/// reads or truncates the underlying Artifact itself.
pub fn project_tool_result_context_v1(
    full_result_ref: ArtifactId,
    full_content_hash: ContentHash,
    full_byte_length: u64,
    canonical_model_visible_envelope: &[u8],
) -> Result<ToolResultContextProjectionV1, ToolResultContextProjectionError> {
    if u64::try_from(canonical_model_visible_envelope.len()).ok() != Some(full_byte_length) {
        return Err(ToolResultContextProjectionError::FullLengthMismatch);
    }
    if ContentHash::blake3(canonical_model_visible_envelope) != full_content_hash {
        return Err(ToolResultContextProjectionError::FullContentHashMismatch);
    }
    let text = std::str::from_utf8(canonical_model_visible_envelope)
        .expect("canonical JSON emitted by Rust is always UTF-8");
    let preview = large_tool_result_preview(text, &full_result_ref.to_string());
    Ok(ToolResultContextProjectionV1 {
        full_result_ref,
        full_content_hash,
        full_byte_length,
        preview: preview.text,
        preview_truncated: !preview.preview_is_complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        sequence: u64,
        atomic_group_id: Option<&str>,
        kind: HistoryEventKind,
        tokens: u64,
    ) -> HistoryEventUnit {
        HistoryEventUnit {
            event_id: EventId::new(),
            sequence: SessionSequence::new(sequence),
            turn_segment_id: "segment".to_owned(),
            history_atomic_group_ref: None,
            atomic_group_id: atomic_group_id.map(ToOwned::to_owned),
            kind,
            estimated_tokens: tokens,
            sealed: true,
        }
    }

    #[test]
    fn an_earlier_addition_can_be_compacted_while_the_root_and_latest_group_stay() {
        let mut root = event(1, None, HistoryEventKind::UserMessage, 10);
        root.sealed = false;
        let addition = event(2, None, HistoryEventKind::UserMessage, 100);
        let mut latest = event(3, None, HistoryEventKind::AssistantMessage, 10);
        latest.sealed = false;
        let plan =
            plan_recent_history(100, 100, 0, &[root.clone(), addition.clone(), latest.clone()])
                .expect("plan");
        assert!(plan.compact_event_ids.contains(&addition.event_id));
        assert!(plan.retained_event_ids.contains(&root.event_id));
        assert!(plan.retained_event_ids.contains(&latest.event_id));
        assert!(!plan.compact_event_ids.contains(&root.event_id));
        assert!(!plan.compact_event_ids.contains(&latest.event_id));
    }

    #[test]
    fn what_stays_leaves_a_quarter_of_the_budget_after_the_parts_no_compaction_removes() {
        let older = event(1, None, HistoryEventKind::UserMessage, 20_000);
        let recent = event(2, None, HistoryEventKind::UserMessage, 20_000);
        let mut current = event(3, None, HistoryEventKind::AssistantMessage, 30_000);
        current.sealed = false;
        let units = [older.clone(), recent.clone(), current.clone()];
        // Budget 100,000: three quarters is 75,000; 30,000 fixed and 30,000 current leave 15,000.
        let plan = plan_recent_history(200_000, 100_000, 30_000, &units).expect("plan");
        assert_eq!(plan.compact_event_ids, [older.event_id, recent.event_id]);
        assert_eq!(plan.retained_event_ids, [current.event_id]);
        // With room for it, the newest sealed message stays.
        let plan = plan_recent_history(200_000, 100_000, 10_000, &units).expect("plan");
        assert_eq!(plan.compact_event_ids, [older.event_id]);
        assert_eq!(plan.retained_recent_tokens, 20_000);
    }

    /// With room to spare, the history kept as it was
    /// is at least 8% of the window and stops at 10%.
    #[test]
    fn what_stays_after_a_compaction_is_eight_to_ten_percent_of_the_window() {
        let units = (1..=8)
            .map(|sequence| event(sequence, None, HistoryEventKind::UserMessage, 4_500))
            .collect::<Vec<_>>();
        let plan = plan_recent_history(100_000, 1_000_000, 0, &units).expect("plan");
        assert_eq!(plan.retained_recent_tokens, 9_000, "two groups: 9%, and a third passes 10%");
        assert_eq!(plan.retained_event_ids, [units[6].event_id, units[7].event_id]);
        assert!(plan.retention_exceptions.is_empty(), "9% is inside the band");
    }

    /// A real DeepSeek run kept 18,714 tokens of a 256k window (7%): two percent
    /// is narrower than one step's group, so the group that passes the band's top before its
    /// floor is reached stays when that leaves what is kept nearer the band than stopping short;
    /// a far larger one does not.
    #[test]
    fn a_group_passing_the_band_stays_when_that_is_nearer_it() {
        let mut units = [7_000, 9_300, 9_400]
            .into_iter()
            .zip(1..)
            .map(|(tokens, sequence)| event(sequence, None, HistoryEventKind::UserMessage, tokens))
            .collect::<Vec<_>>();
        // 8% of 256,000 is 20,480 and 10% is 25,600: two groups keep 18,700, three 25,700.
        let plan = plan_recent_history(256_000, 1_000_000, 0, &units).expect("plan");
        assert_eq!(plan.retained_recent_tokens, 25_700);
        assert!(plan.retention_exceptions.is_empty());
        units[0] = event(1, None, HistoryEventKind::UserMessage, 60_000);
        let plan = plan_recent_history(256_000, 1_000_000, 0, &units).expect("plan");
        assert_eq!(plan.retained_recent_tokens, 18_700, "78,700 would be far past the band");
        assert_eq!(
            plan.retention_exceptions,
            [
                ContextRetentionException::OversizedAtomicSegment,
                ContextRetentionException::InsufficientEligibleRecentHistory
            ]
        );
    }

    #[test]
    fn compacting_all_sealed_history_keeps_only_the_unsealed_groups() {
        let mut root = event(1, None, HistoryEventKind::UserMessage, 10);
        root.sealed = false;
        let earlier = event(2, None, HistoryEventKind::UserMessage, 10);
        let mut latest = event(3, None, HistoryEventKind::AssistantMessage, 10);
        latest.sealed = false;
        let plan = plan_compact_all_sealed(&[root.clone(), earlier.clone(), latest.clone()])
            .expect("plan");
        assert_eq!(plan.compact_event_ids, [earlier.event_id]);
        assert_eq!(plan.retained_event_ids, [root.event_id, latest.event_id]);
        assert_eq!(plan.compacted_tokens, 10);
    }

    #[test]
    fn tool_call_and_result_atomic_group_is_never_split() {
        let tool_call_id = ToolCallId::new();
        let events = vec![
            event(1, None, HistoryEventKind::UserMessage, 2_000),
            event(2, Some("tool"), HistoryEventKind::ToolCall { tool_call_id }, 1_000),
            event(3, Some("tool"), HistoryEventKind::ToolResult { tool_call_id }, 9_000),
            event(4, None, HistoryEventKind::AssistantMessage, 3_000),
        ];
        let plan = plan_recent_history(32_000, 20_000, 10_000, &events).expect("plan");
        let tool_retained =
            events[1..=2].iter().all(|event| plan.retained_event_ids.contains(&event.event_id));
        let tool_compacted =
            events[1..=2].iter().all(|event| plan.compact_event_ids.contains(&event.event_id));
        assert!(tool_retained || tool_compacted);
    }

    #[test]
    fn large_tool_result_preview_preserves_unicode_character_boundaries() {
        let text = "界".repeat(9_000);
        let preview = large_tool_result_preview(&text, "art_fixture");
        assert!(!preview.preview_is_complete);
        assert!(preview.text.contains("middle pruned"));
        assert!(preview.text.ends_with("Original artifact: art_fixture"));
    }

    #[test]
    fn tool_result_context_projection_binds_the_full_artifact_fact() {
        let bytes = br#"{"schema_version":1,"value":"safe"}"#;
        let artifact_id = ArtifactId::new();
        let hash = ContentHash::blake3(bytes);
        let projection = project_tool_result_context_v1(
            artifact_id,
            hash.clone(),
            u64::try_from(bytes.len()).expect("length"),
            bytes,
        )
        .expect("projection");
        assert_eq!(projection.full_result_ref, artifact_id);
        assert_eq!(projection.full_content_hash, hash);
        assert!(!projection.preview_truncated);
        assert_eq!(projection.preview, std::str::from_utf8(bytes).expect("utf8"));
    }

    #[test]
    fn tool_result_context_projection_rejects_mismatched_artifact_facts() {
        let bytes = b"{}";
        assert_eq!(
            project_tool_result_context_v1(
                ArtifactId::new(),
                ContentHash::blake3(b"different"),
                2,
                bytes,
            ),
            Err(ToolResultContextProjectionError::FullContentHashMismatch)
        );
    }
}
