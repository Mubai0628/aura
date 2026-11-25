use crate::{MemoryHistory, MemoryRead, MemoryVaultRequest};
use aura_domain_common::{MemoryId, SessionId, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryAuthorStamp {
    pub actor: String,
    pub session_id: Option<SessionId>,
    pub session_name_at_write: Option<String>,
    pub at: UtcTimestamp,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryProvenance {
    /// None means there is no reliable creation record. Source sessions alone are not proof.
    pub creator: Option<MemoryAuthorStamp>,
    /// New authorship stamps plus recent known events, most recent last.
    pub modifiers: Vec<MemoryAuthorStamp>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryTimelineQuery {
    pub author_session_id: Option<SessionId>,
    pub since: Option<UtcTimestamp>,
    pub until: Option<UtcTimestamp>,
    /// Current category of the note; deleted/unindexed notes have no category.
    pub category: Option<String>,
    /// The record hash, not a row number tied to one SQLite rebuild.
    pub before: Option<String>,
    #[schemars(range(min = 1, max = 100))]
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryTimelineRequest {
    pub vault: MemoryVaultRequest,
    pub query: MemoryTimelineQuery,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryTimelineEntry {
    pub cursor: String,
    pub event: MemoryHistory,
    pub category: Option<String>,
    pub session_name_at_write: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryTimeline {
    pub entries: Vec<MemoryTimelineEntry>,
    pub before: Option<String>,
    pub incomplete: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryBatchReadRequest {
    pub vault: MemoryVaultRequest,
    #[schemars(length(min = 1, max = 100))]
    pub ids: Vec<MemoryId>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryBatchRead {
    pub notes: Vec<MemoryRead>,
    pub missing_ids: Vec<MemoryId>,
}
