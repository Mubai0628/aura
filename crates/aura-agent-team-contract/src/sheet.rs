//! The task sheet: everything a sub-agent is told about its task. It sees nothing else of the
//! conversation that started it.
use crate::Refusal;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const TEXT_MAX_CHARS: usize = 8_000;
const SHEET_MAX_CHARS: usize = 20_000;
const LIST_MAX: usize = 50;

/// The task a sub-agent works on.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TaskSheet {
    /// What the sub-agent is to find out or do.
    #[schemars(length(max = 8000), regex(pattern = r"\S"))]
    pub goal: String,
    /// When the task counts as done: what its report must contain.
    #[schemars(length(max = 8000), regex(pattern = r"\S"))]
    pub done_when: String,
    /// The part of the project the task covers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Rules the work keeps to.
    #[schemars(length(max = 50))]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
    /// Files to read first, relative to the workspace.
    #[schemars(length(max = 50))]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// Facts from this conversation the sub-agent needs; it sees nothing else of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
}

impl TaskSheet {
    /// Refuses an empty or oversized sheet and one that names a conversation.
    pub fn check(&self) -> Result<(), Refusal> {
        if self.goal.trim().is_empty() || self.done_when.trim().is_empty() {
            return Err(Refusal::new(
                "agents.task_incomplete",
                "任务单要写目标（goal）和做到什么算完（done_when）。",
            ));
        }
        if self.constraints.len() > LIST_MAX || self.files.len() > LIST_MAX {
            return Err(Refusal::new(
                "agents.task_too_long",
                format!("约束和文件各最多 {LIST_MAX} 条。"),
            ));
        }
        let mut total = 0;
        for text in self.texts() {
            let length = text.chars().count();
            total += length;
            if length > TEXT_MAX_CHARS || total > SHEET_MAX_CHARS {
                return Err(Refusal::new(
                    "agents.task_too_long",
                    format!(
                        "任务单每项最多 {TEXT_MAX_CHARS} 字，合计最多 {SHEET_MAX_CHARS} 字；把要点写进去，细节让子 Agent 自己读文件。"
                    ),
                ));
            }
            if let Some(reference) = session_reference(text) {
                return Err(Refusal::new(
                    "agents.task_names_conversation",
                    format!(
                        "任务单里有会话引用 {reference}：子 Agent 不能读别的会话。把需要的事实直接写进任务单的 background。"
                    ),
                ));
            }
        }
        Ok(())
    }

    fn texts(&self) -> impl Iterator<Item = &str> {
        [Some(self.goal.as_str()), Some(self.done_when.as_str()), self.scope.as_deref()]
            .into_iter()
            .flatten()
            .chain(self.constraints.iter().map(String::as_str))
            .chain(self.files.iter().map(String::as_str))
            .chain(self.background.as_deref())
    }

    /// The sheet as the sub-agent's first message.
    #[must_use]
    pub fn render(&self, name: &str) -> String {
        let mut text = format!("来自主 Agent 的任务单：{name}\n\n## 目标\n{}\n", self.goal.trim());
        text.push_str(&format!("\n## 做到什么算完\n{}\n", self.done_when.trim()));
        if let Some(scope) = self.scope.as_deref().filter(|s| !s.trim().is_empty()) {
            text.push_str(&format!("\n## 范围\n{}\n", scope.trim()));
        }
        let list = |title: &str, items: &[String]| {
            if items.is_empty() {
                return String::new();
            }
            let lines: Vec<String> =
                items.iter().map(|item| format!("- {}", item.trim())).collect();
            format!("\n## {title}\n{}\n", lines.join("\n"))
        };
        text.push_str(&list("约束", &self.constraints));
        text.push_str(&list("要看的文件", &self.files));
        if let Some(background) = self.background.as_deref().filter(|s| !s.trim().is_empty()) {
            text.push_str(&format!("\n## 背景\n{}\n", background.trim()));
        }
        text
    }
}

/// The first conversation reference in `text`: a handle (`conv_` and 16 hex digits) or a
/// conversation's own id (`ses_` and 26 ULID characters).
#[must_use]
pub fn session_reference(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let found = |prefix: &str, length: usize, valid: fn(u8) -> bool| {
        text.match_indices(prefix).find_map(|(start, _)| {
            let begins = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
            let body = start + prefix.len();
            let end = body + length;
            let whole = bytes.get(body..end)?.iter().all(|b| valid(*b))
                && bytes.get(end).is_none_or(|b| !b.is_ascii_alphanumeric());
            (begins && whole).then(|| &text[start..end])
        })
    };
    found("conv_", 16, |b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        .or_else(|| found("ses_", 26, |b| b.is_ascii_alphanumeric()))
}
