use aura_domain_common::{MemoryId, SessionId, UtcTimestamp, WorkspaceId, WorkspacePath};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const MAX_NOTE_BYTES: usize = 1_048_576;
pub const MAX_BATCH: usize = 100;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Global,
    Project,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VaultScope {
    Global,
    Project { workspace_id: WorkspaceId },
}

impl VaultScope {
    pub const fn scope(self) -> MemoryScope {
        match self {
            Self::Global => MemoryScope::Global,
            Self::Project { .. } => MemoryScope::Project,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Superseded,
    Forgotten,
    Conflicted,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryMetadata {
    pub schema_version: u32,
    pub id: MemoryId,
    pub scope: MemoryScope,
    pub category: String,
    pub status: MemoryStatus,
    pub confidence: f64,
    pub importance: u8,
    pub recall_weight: f64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub last_confirmed_at: Option<UtcTimestamp>,
    pub source_sessions: Vec<SessionId>,
    pub source_events: Vec<String>,
    pub tags: Vec<String>,
    pub related_ids: Vec<MemoryId>,
    pub supersedes_ids: Vec<MemoryId>,
    pub conflicts_with_ids: Vec<MemoryId>,
    /// Flat properties outside Aura's schema, such as Obsidian `aliases`, kept
    /// verbatim (one entry per property, list items included) and written back
    /// after Aura's fields. They are the user's: never interpreted or sent anywhere.
    #[serde(skip)]
    pub user_properties: Vec<String>,
}

impl MemoryMetadata {
    pub fn new(scope: MemoryScope, category: String) -> Self {
        let now = UtcTimestamp::now();
        Self {
            schema_version: 1,
            id: MemoryId::new(),
            scope,
            category,
            status: MemoryStatus::Active,
            confidence: 1.0,
            importance: 3,
            recall_weight: 1.0,
            created_at: now,
            updated_at: now,
            last_confirmed_at: None,
            source_sessions: vec![],
            source_events: vec![],
            tags: vec![],
            related_ids: vec![],
            supersedes_ids: vec![],
            conflicts_with_ids: vec![],
            user_properties: vec![],
        }
    }
    pub fn validate(&self) -> Result<(), MemoryFormatError> {
        if self.schema_version != 1 {
            return Err(MemoryFormatError::Invalid("schema_version"));
        }
        if self.category.is_empty()
            || self.category.len() > 64
            || !self
                .category
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err(MemoryFormatError::Invalid("category"));
        }
        if !(1..=5).contains(&self.importance) {
            return Err(MemoryFormatError::Invalid("importance"));
        }
        if !self.confidence.is_finite()
            || !(0.0..=1.0).contains(&self.confidence)
            || !self.recall_weight.is_finite()
            || self.recall_weight < 0.0
        {
            return Err(MemoryFormatError::Invalid("recall weights"));
        }
        if self
            .related_ids
            .iter()
            .chain(&self.supersedes_ids)
            .chain(&self.conflicts_with_ids)
            .any(|id| *id == self.id)
        {
            return Err(MemoryFormatError::Invalid("self relation"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryDocument {
    pub metadata: MemoryMetadata,
    pub body: String,
}

impl MemoryDocument {
    pub fn title(&self) -> String {
        self.body
            .lines()
            .find_map(|l| l.trim().strip_prefix("# "))
            .unwrap_or("未命名记忆")
            .chars()
            .take(200)
            .collect()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MemoryFormatError {
    #[error("invalid memory field: {0}")]
    Invalid(&'static str),
    #[error("invalid flat YAML frontmatter at line {0}")]
    Frontmatter(usize),
    #[error("memory note exceeds the size limit")]
    TooLarge,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemorySelector {
    Id { id: MemoryId },
    Path { scope: MemoryScope, path: WorkspacePath },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryHistory {
    pub id: Option<MemoryId>,
    pub path: String,
    pub action: String,
    pub actor: String,
    pub session_id: Option<SessionId>,
    pub turn_id: Option<String>,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
    pub title: String,
    pub at: UtcTimestamp,
}
