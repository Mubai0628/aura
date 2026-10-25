//! When a request does not fit, the results of tool calls older than the latest
//! six steps give their room back before any history is summarized: each becomes one line naming
//! the tool, its size and how to read it again. All of them are cleared together, and only when
//! they free at least 20,000 tokens, so the request's prefix changes rarely; a result once cleared
//! stays cleared.
use std::collections::BTreeSet;

use aura_domain_common::{ContentHash, ToolCallId};
use aura_model_domain::LogicalConversationItem;

/// The steps whose tool results always stay as they were.
pub const KEPT_TOOL_STEPS: usize = 6;
/// A result smaller than this is not worth a placeholder.
pub const CLEARED_RESULT_MIN_TOKENS: u64 = 1_000;
/// A batch frees at least this much, or nothing is cleared.
pub const CLEARING_BATCH_MIN_TOKENS: u64 = 20_000;
/// A result of a batch on its own, which may go once
/// the model has seen it, within the latest steps too - when a compaction pass has not made the
/// request fit (item 8, part 6: "一轮压缩腾不出空间时……先处理超大段").
pub const OVERSIZED_RESULT_TOKENS: u64 = CLEARING_BATCH_MIN_TOKENS;
const MARK: &str = "[Earlier tool result cleared to save context:";

/// The line a cleared result is shown as.
#[must_use]
pub fn cleared_tool_result(tool_name: &str, characters: usize, tool_call_id: ToolCallId) -> String {
    format!(
        "{MARK} {tool_name}, {characters} characters. Read it again with context.read_tool_result, \
         tool_call_id {tool_call_id}.]"
    )
}

#[must_use]
pub fn is_cleared_tool_result(content: &str) -> bool {
    content.starts_with(MARK)
}

/// The results these items show cleared — those of the previous request, which stay cleared.
#[must_use]
pub fn cleared_tool_results(items: &[LogicalConversationItem]) -> BTreeSet<ToolCallId> {
    items
        .iter()
        .filter_map(|item| match item {
            LogicalConversationItem::ToolResult { tool_call_id, content, .. }
                if is_cleared_tool_result(content) =>
            {
                Some(*tool_call_id)
            }
            _ => None,
        })
        .collect()
}

/// Shows the named results cleared; returns how many changed.
pub fn clear_tool_results(
    items: &mut [LogicalConversationItem],
    ids: &BTreeSet<ToolCallId>,
) -> usize {
    let mut cleared = 0;
    for item in items {
        if let LogicalConversationItem::ToolResult {
            tool_call_id,
            tool_name,
            content,
            content_hash,
            ..
        } = item
            && ids.contains(tool_call_id)
            && !is_cleared_tool_result(content)
        {
            *content = cleared_tool_result(tool_name, content.chars().count(), *tool_call_id);
            *content_hash = ContentHash::sha256(content.as_bytes());
            cleared += 1;
        }
    }
    cleared
}

/// The next batch to clear: every result older than the latest [`KEPT_TOOL_STEPS`] steps and
/// larger than [`CLEARED_RESULT_MIN_TOKENS`], when together they free
/// [`CLEARING_BATCH_MIN_TOKENS`] — or none. All of them go at once: the real DeepSeek run showed
/// 20,000 taken oldest first lasting less than two steps near the line, each batch moving the
/// prefix. Bytes are counted as tokens at `rate` (tokens, bytes), the rate the request's own
/// estimate used. With `oversized`, a seen result of [`OVERSIZED_RESULT_TOKENS`] may go from the
/// latest steps too.
#[must_use]
pub fn next_clearing_batch(
    items: &[LogicalConversationItem],
    rate: (u64, u64),
    oversized: bool,
) -> BTreeSet<ToolCallId> {
    let steps = items
        .iter()
        .enumerate()
        .filter(|(_, item)| matches!(item, LogicalConversationItem::Assistant { .. }))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    // The latest step's results have not been seen yet and always stay.
    let Some(&latest) = steps.last() else {
        return BTreeSet::new();
    };
    let first_kept = steps.len().checked_sub(KEPT_TOOL_STEPS).map_or(0, |index| steps[index]);
    let tokens = |bytes: usize| (bytes as u64).saturating_mul(rate.0).div_ceil(rate.1.max(1));
    let mut batch = BTreeSet::new();
    let mut freed = 0_u64;
    for (index, item) in items[..latest].iter().enumerate() {
        let LogicalConversationItem::ToolResult { tool_call_id, tool_name, content, .. } = item
        else {
            continue;
        };
        let size = tokens(content.len());
        if is_cleared_tool_result(content)
            || size <= CLEARED_RESULT_MIN_TOKENS
            || (index >= first_kept && (!oversized || size < OVERSIZED_RESULT_TOKENS))
        {
            continue;
        }
        let line = cleared_tool_result(tool_name, content.chars().count(), *tool_call_id);
        freed += tokens(content.len()).saturating_sub(tokens(line.len()));
        batch.insert(*tool_call_id);
    }
    if freed >= CLEARING_BATCH_MIN_TOKENS { batch } else { BTreeSet::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::EventId;
    use aura_model_domain::LogicalHistoricalToolCall;

    /// One step: the model's call and its result of `size` bytes.
    fn step(size: usize) -> (ToolCallId, [LogicalConversationItem; 2]) {
        let id = ToolCallId::new();
        let content = "x".repeat(size);
        let call = LogicalHistoricalToolCall {
            tool_call_id: id,
            tool_id: None,
            canonical_name: "fs.read".into(),
            arguments_json: "{}".into(),
            arguments_hash: ContentHash::sha256(b"{}"),
        };
        (
            id,
            [
                LogicalConversationItem::Assistant {
                    source_event_id: EventId::new(),
                    content: None,
                    content_hash: None,
                    tool_calls: vec![call],
                    provider_history: None,
                },
                LogicalConversationItem::ToolResult {
                    source_event_id: EventId::new(),
                    tool_call_id: id,
                    tool_name: "fs.read".into(),
                    content_hash: ContentHash::sha256(content.as_bytes()),
                    content,
                    is_error: false,
                },
            ],
        )
    }

    fn conversation(sizes: &[usize]) -> (Vec<ToolCallId>, Vec<LogicalConversationItem>) {
        let mut ids = Vec::new();
        let mut items = Vec::new();
        for size in sizes {
            let (id, pair) = step(*size);
            ids.push(id);
            items.extend(pair);
        }
        (ids, items)
    }

    #[test]
    fn the_latest_six_steps_keep_their_results() {
        // Below the oversized size; an oversized one the model has seen may go (below).
        let (_, items) = conversation(&[19_000; 6]);
        assert!(next_clearing_batch(&items, (1, 1), false).is_empty());
    }

    /// In a real DeepSeek run, batches of 20,000 taken oldest first lasted less than
    /// two steps near the line, and each one moved the prefix. Every older result goes at once.
    #[test]
    fn every_older_result_is_cleared_at_once_when_together_they_free_twenty_thousand() {
        let (ids, items) = conversation(&[8_000, 500, 8_000, 8_000, 8_000, 1, 1, 1, 1, 1, 1]);
        let batch = next_clearing_batch(&items, (1, 1), false);
        // 500 is too small; the four of 8,000 go together.
        assert_eq!(batch, BTreeSet::from([ids[0], ids[2], ids[3], ids[4]]));
        let (_, items) = conversation(&[8_000, 8_000, 1, 1, 1, 1, 1, 1]);
        assert!(
            next_clearing_batch(&items, (1, 1), false).is_empty(),
            "less than a batch clears nothing"
        );
    }

    #[test]
    fn a_result_once_cleared_stays_cleared_and_is_not_counted_again() {
        let (ids, mut items) = conversation(&[8_000, 8_000, 8_000, 8_000, 1, 1, 1, 1, 1, 1]);
        let batch = next_clearing_batch(&items, (1, 1), false);
        assert_eq!(clear_tool_results(&mut items, &batch), 4);
        assert_eq!(cleared_tool_results(&items), batch);
        let LogicalConversationItem::ToolResult { content, content_hash, .. } = &items[1] else {
            panic!("a result")
        };
        assert!(
            content.contains("context.read_tool_result") && content.contains(&ids[0].to_string())
        );
        assert_eq!(*content_hash, ContentHash::sha256(content.as_bytes()));
        assert_eq!(clear_tool_results(&mut items, &batch), 0, "already cleared");
        assert!(next_clearing_batch(&items, (1, 1), false).is_empty(), "nothing older is left");
    }

    /// A result of at least a batch on its own that
    /// the model has already seen gives its room back within the latest six steps too; the latest
    /// step's results stay, however large.
    #[test]
    fn an_oversized_result_the_model_has_seen_is_cleared_within_the_latest_steps() {
        // At a quarter of a token a byte, 100,000 bytes are 25,000 tokens.
        let (ids, items) = conversation(&[100_000, 8_000, 100_000]);
        assert_eq!(next_clearing_batch(&items, (1, 4), true), BTreeSet::from([ids[0]]));
        // Before a compaction pass has tried, the latest six steps keep it.
        assert!(next_clearing_batch(&items, (1, 4), false).is_empty());
    }

    #[test]
    fn bytes_are_counted_at_the_requests_own_rate() {
        // At a quarter of a token a byte, 8,000 bytes are 2,000 tokens: eleven free a batch; ten
        // do not, each line that stays in their place keeping a few tokens.
        let mut sizes = vec![8_000; 11];
        sizes.extend([1; 6]);
        let (ids, items) = conversation(&sizes);
        assert_eq!(next_clearing_batch(&items, (1, 4), false), ids[..11].iter().copied().collect());
        let mut sizes = vec![8_000; 10];
        sizes.extend([1; 6]);
        assert!(next_clearing_batch(&conversation(&sizes).1, (1, 4), false).is_empty());
    }
}
