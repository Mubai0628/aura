//! The conversation's history by round, as the model looks it up. A round (T1, T2…) is one task:
//! from the user's request until it was completed. Questions and their answers, messages the user
//! added and messages after a stop or an error belong to the same round; the message after a
//! completed task begins the next one. Each round's entries (T3.1, T3.2…) are, in order, the
//! user's messages, the model's thinking where its text was kept, the model's replies and its tool
//! calls with their results.
use aura_domain_common::UtcTimestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The first characters of a round's first message a listing shows.
pub const HISTORY_FIRST_MESSAGE_CHARS: usize = 200;
/// The characters on each side of a search hit.
pub const HISTORY_SNIPPET_SIDE_CHARS: usize = 30;

/// Which rounds to list. At most one of `last`, `first`, `ids` or a time range; with none, the
/// last 10.
#[derive(Clone, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryListTurnsInput {
    /// The newest n rounds, 1 to 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 50))]
    pub last: Option<u32>,
    /// The oldest n rounds, 1 to 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 50))]
    pub first: Option<u32>,
    /// These rounds, by id (T3), at most 50.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    /// Rounds whose first message was sent at or after this time (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<UtcTimestamp>,
    /// Rounds whose first message was sent before this time (RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<UtcTimestamp>,
}

/// One round in a listing.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryTurnLine {
    /// The round's id: T5.
    pub id: String,
    /// When its first message was sent.
    pub sent_at: UtcTimestamp,
    /// The first message's first 200 characters; read the rest with history.read (T5.1).
    pub first_message: String,
    /// The first message's whole length in characters.
    pub first_message_chars: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryListTurnsResult {
    pub turns: Vec<HistoryTurnLine>,
    /// How many rounds the conversation has.
    pub total: u32,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum HistoryEntryKind {
    UserMessage,
    ModelThinking,
    ModelOutput,
    ToolCall,
}

/// What to look for in the conversation's own history.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistorySearchInput {
    /// Words that must all appear in an entry, ignoring case.
    pub query: String,
    /// Only these rounds (T3); the whole conversation when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turns: Option<Vec<String>>,
    /// Only entries of these kinds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinds: Option<Vec<HistoryEntryKind>>,
    /// At most this many hits, 1 to 30; default 10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 30))]
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistorySearchHit {
    /// The entry, for history.read: T3.17.
    pub entry: String,
    pub kind: HistoryEntryKind,
    /// The tool a tool call entry called.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    pub at: UtcTimestamp,
    /// About 30 characters on each side of the first word found.
    pub snippet: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistorySearchResult {
    /// Newest first.
    pub hits: Vec<HistorySearchHit>,
    /// False when the search stopped before it reached the oldest round asked for (time or the
    /// hit limit); `searched_back_to` says how far it got.
    pub complete: bool,
    /// The oldest round searched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub searched_back_to: Option<String>,
}

/// Entries to read whole, by id, a page of each.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryReadInput {
    /// Entry ids (T3.17), 1 to 10.
    pub entries: Vec<String>,
    /// The character each entry's text starts from; default 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    /// Characters of each entry's text, 1 to 64000; default 8000.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 64000))]
    pub limit: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntryView {
    pub entry: String,
    pub kind: HistoryEntryKind,
    pub at: UtcTimestamp,
    /// The tool a tool call entry called.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// A tool call's arguments, as it sent them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    /// The text from `offset`: the message, the thinking, the reply, or the tool's result.
    pub text: String,
    /// The text's whole length in characters.
    pub chars: u64,
    pub offset: u64,
    /// Where the next page starts, while the text goes on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_offset: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryReadResult {
    pub entries: Vec<HistoryEntryView>,
}

/// `T5` as a round number.
#[must_use]
pub fn history_round_number(id: &str) -> Option<u32> {
    let digits = id.trim().strip_prefix(['T', 't'])?;
    (!digits.is_empty() && digits.len() <= 9 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then(|| digits.parse().ok())
        .flatten()
        .filter(|number| *number > 0)
}

/// `T5.17` as a round and an entry number.
#[must_use]
pub fn history_entry_number(id: &str) -> Option<(u32, u32)> {
    let (round, entry) = id.trim().split_once('.')?;
    let entry =
        (!entry.is_empty() && entry.len() <= 9 && entry.bytes().all(|b| b.is_ascii_digit()))
            .then(|| entry.parse::<u32>().ok())
            .flatten()
            .filter(|number| *number > 0)?;
    Some((history_round_number(round)?, entry))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_and_entry_ids_read_only_as_written() {
        assert_eq!(history_round_number("T12"), Some(12));
        assert_eq!(history_round_number(" t3 "), Some(3));
        assert_eq!(history_round_number("T0"), None);
        assert_eq!(history_round_number("3"), None);
        assert_eq!(history_round_number("T3.1"), None);
        assert_eq!(history_entry_number("T3.17"), Some((3, 17)));
        assert_eq!(history_entry_number("T3."), None);
        assert_eq!(history_entry_number("T3.0"), None);
        assert_eq!(history_entry_number("3.17"), None);
    }
}
