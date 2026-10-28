//! A round's entries, numbered in order: each user message; for each model response its thinking
//! (where its text was kept), its reply and each tool call with its result.
use super::rows::{Body, Row};
use aura_context_domain::{HistoryEntryKind, HistoryEntryView};
use aura_domain_common::UtcTimestamp;

/// What a tool call entry shows while the call has no result.
const NO_RESULT: &str = "（这次调用还没有结果）";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Entry {
    pub number: u32,
    pub kind: HistoryEntryKind,
    pub at: UtcTimestamp,
    pub tool: Option<String>,
    pub arguments: Option<String>,
    pub text: String,
}

pub(super) fn of(rows: &[Row]) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut push = |kind, at, tool, arguments, text| {
        let number = u32::try_from(entries.len() + 1).unwrap_or(u32::MAX);
        entries.push(Entry { number, kind, at, tool, arguments, text });
    };
    for row in rows {
        match &row.body {
            Body::User(text) => {
                push(HistoryEntryKind::UserMessage, row.at, None, None, text.clone())
            }
            Body::Model { thinking, reply, calls } => {
                if !thinking.is_empty() {
                    push(HistoryEntryKind::ModelThinking, row.at, None, None, thinking.clone());
                }
                if !reply.is_empty() {
                    push(HistoryEntryKind::ModelOutput, row.at, None, None, reply.clone());
                }
                for call in calls {
                    push(
                        HistoryEntryKind::ToolCall,
                        row.at,
                        Some(call.tool.clone()),
                        Some(call.arguments.clone()),
                        call.result.clone().unwrap_or_else(|| NO_RESULT.to_owned()),
                    );
                }
            }
        }
    }
    entries
}

impl Entry {
    #[must_use]
    pub fn id(&self, round: u32) -> String {
        format!("T{round}.{}", self.number)
    }

    /// What a search looks in: the text, and a tool call's name and arguments before its result.
    pub(super) fn haystack(&self) -> String {
        match (&self.tool, &self.arguments) {
            (Some(tool), Some(arguments)) => format!("{tool} {arguments}\n{}", self.text),
            _ => self.text.clone(),
        }
    }

    /// The entry with a page of its text: `limit` characters from `offset`.
    pub(super) fn view(&self, round: u32, offset: u64, limit: u64) -> HistoryEntryView {
        let chars = self.text.chars().count() as u64;
        let start = offset.min(chars);
        let text: String = self
            .text
            .chars()
            .skip(usize::try_from(start).unwrap_or(usize::MAX))
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect();
        let end = start + text.chars().count() as u64;
        HistoryEntryView {
            entry: self.id(round),
            kind: self.kind,
            at: self.at,
            tool: self.tool.clone(),
            arguments: self.arguments.clone(),
            text,
            chars,
            offset: start,
            next_offset: (end < chars).then_some(end),
        }
    }
}
