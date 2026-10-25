use crate::MemoryScope;
use aura_domain_common::{
    MemoryId, ModelProfileId, RuntimeInstanceId, SessionId, UtcTimestamp, WorkspacePath,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryCandidateContent {
    pub category: String,
    pub body: String,
    pub basis: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 0 to 1: how sure the note is.
    #[schemars(range(min = 0, max = 1))]
    pub confidence: Option<f64>,
    /// 1 to 5: 3 is ordinary, 5 is critical.
    #[schemars(range(min = 1, max = 5))]
    pub importance: Option<u8>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemoryCandidate {
    Create {
        scope: MemoryScope,
        path: WorkspacePath,
        content: MemoryCandidateContent,
    },
    Update {
        scope: MemoryScope,
        target: MemoryId,
        expected_hash: String,
        content: MemoryCandidateContent,
    },
    Supersede {
        scope: MemoryScope,
        target: MemoryId,
        expected_hash: String,
        path: WorkspacePath,
        content: MemoryCandidateContent,
    },
    Conflict {
        scope: MemoryScope,
        target: MemoryId,
        expected_hash: String,
        path: WorkspacePath,
        content: MemoryCandidateContent,
    },
    Ignore {
        scope: MemoryScope,
        basis: String,
    },
}
impl MemoryCandidate {
    pub fn scope(&self) -> MemoryScope {
        match self {
            Self::Create { scope, .. }
            | Self::Update { scope, .. }
            | Self::Supersede { scope, .. }
            | Self::Conflict { scope, .. }
            | Self::Ignore { scope, .. } => *scope,
        }
    }
    pub fn content(&self) -> Option<&MemoryCandidateContent> {
        match self {
            Self::Create { content, .. }
            | Self::Update { content, .. }
            | Self::Supersede { content, .. }
            | Self::Conflict { content, .. } => Some(content),
            Self::Ignore { .. } => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryEventCandidate {
    pub title: String,
    pub summary: String,
    pub basis: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryMaintenanceOutput {
    pub schema_version: u32,
    pub candidates: Vec<MemoryCandidate>,
    pub events: Vec<MemoryEventCandidate>,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMaintenanceState {
    Queued,
    Running,
    Completed,
    Skipped,
    Failed,
    Interrupted,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryMaintenanceRun {
    pub run_id: String,
    pub runtime_instance_id: RuntimeInstanceId,
    pub session_id: SessionId,
    pub turn_id: Option<String>,
    pub state: MemoryMaintenanceState,
    pub reason: Option<String>,
    pub model_profile_id: Option<ModelProfileId>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Counts observed so far; false means final Provider usage is unconfirmed.
    #[serde(default)]
    pub usage_complete: bool,
    pub started_at: UtcTimestamp,
    pub finished_at: Option<UtcTimestamp>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryMaintenanceResult {
    pub created: u32,
    pub updated: u32,
    pub ignored: u32,
    pub events: u32,
}

/// Parse a model's maintenance answer. Models often wrap a JSON answer in one
/// Markdown code fence; exactly that shape is unwrapped, nothing else is
/// repaired, and the contract is validated afterwards as before.
pub fn parse_maintenance_output(text: &str) -> Result<MemoryMaintenanceOutput, serde_json::Error> {
    let trimmed = text.trim();
    let body = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .and_then(|rest| rest.strip_suffix("```"))
        .map_or(trimmed, str::trim);
    serde_json::from_str(body)
}

/// 12 of the owner's 37 maintenance
/// runs failed; since an importance outside 1 to 5 is ignored, five still failed on one
/// candidate - a category outside lowercase letters, digits and hyphens, or an expected hash that
/// is not one - and that candidate discarded every other. A category is written the way the store
/// keeps it; a candidate that names no real hash is left out. Returns how many were left out.
pub fn normalize_maintenance_output(output: &mut MemoryMaintenanceOutput) -> usize {
    let before = output.candidates.len();
    output.candidates.retain(|candidate| match candidate {
        MemoryCandidate::Update { expected_hash, .. }
        | MemoryCandidate::Supersede { expected_hash, .. }
        | MemoryCandidate::Conflict { expected_hash, .. } => {
            expected_hash.len() == 64
                && expected_hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        _ => true,
    });
    for candidate in &mut output.candidates {
        let content = match candidate {
            MemoryCandidate::Create { content, .. }
            | MemoryCandidate::Update { content, .. }
            | MemoryCandidate::Supersede { content, .. }
            | MemoryCandidate::Conflict { content, .. } => content,
            MemoryCandidate::Ignore { .. } => continue,
        };
        content.category = normalized_category(&content.category);
    }
    output.candidates.truncate(20);
    output.events.truncate(20);
    before - output.candidates.len()
}

/// A category as the store keeps it: lowercase letters, digits and single hyphens, at most 64.
fn normalized_category(category: &str) -> String {
    let mut out = String::new();
    for ch in category.trim().chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_end_matches('-');
    let out: String = out.chars().take(64).collect();
    let out = out.trim_end_matches('-');
    if out.is_empty() { "project".into() } else { out.into() }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryWorkEvidence {
    pub normally_completed: bool,
    pub side_effect_calls: u32,
    pub tool_calls: u32,
    pub answer_characters: usize,
}
impl MemoryWorkEvidence {
    pub fn trigger(&self) -> Option<&'static str> {
        if !self.normally_completed || self.tool_calls == 0 {
            return None;
        }
        if self.side_effect_calls > 0 {
            Some("有实际副作用的工具调用")
        } else if self.tool_calls >= 8 {
            Some("本轮工具调用达到 8 次")
        } else if self.answer_characters >= 2000 {
            Some("工具辅助的回答达到 2000 字符")
        } else {
            None
        }
    }
}

#[cfg(test)]
#[path = "maintenance_tests.rs"]
mod tests;
