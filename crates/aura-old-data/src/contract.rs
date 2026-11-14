//! What 设置 → 系统 → 存储 shows of old data and sends to remove it (`storage.old_data.*`).
use aura_domain_common::{UtcTimestamp, display_path};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{Kind, OldData, Removed};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OldDataListing {
    pub items: Vec<OldDataEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OldDataEntry {
    /// What a removal names it by; it stays the same while the item is there.
    pub id: String,
    pub kind: OldDataKind,
    /// The folders and files removing it deletes.
    pub paths: Vec<String>,
    pub bytes: u64,
    /// The newest change to anything in it.
    pub modified_at: Option<UtcTimestamp>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OldDataKind {
    /// What the versions before namespaces kept at the top of Aura's folders.
    EarlyLayout,
    /// The data and settings of a namespace no installation uses.
    Namespace,
    /// Folders whose removal began and did not finish.
    Unfinished,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OldDataRemoveRequest {
    /// The ids of the items to remove, from the listing.
    pub ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OldDataRemoval {
    pub removed: Vec<String>,
    /// Kept whole: a file in it is open, or a program works in it.
    pub in_use: Vec<String>,
    pub failed: Vec<OldDataFailure>,
    /// What is left over after the removal.
    pub items: Vec<OldDataEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OldDataFailure {
    pub id: String,
    pub reason: String,
}

impl From<&OldData> for OldDataEntry {
    fn from(item: &OldData) -> Self {
        Self {
            id: item.id.clone(),
            kind: match item.kind {
                Kind::EarlyLayout => OldDataKind::EarlyLayout,
                Kind::Namespace => OldDataKind::Namespace,
                Kind::Unfinished => OldDataKind::Unfinished,
            },
            paths: item.paths.iter().map(|path| display_path(path)).collect(),
            bytes: item.bytes,
            modified_at: item
                .modified
                .map(|modified| UtcTimestamp::from_value(OffsetDateTime::from(modified))),
        }
    }
}

impl OldDataListing {
    #[must_use]
    pub fn of(items: &[OldData]) -> Self {
        Self { items: items.iter().map(OldDataEntry::from).collect() }
    }
}

impl OldDataRemoval {
    /// What `removed` did, with what is left.
    #[must_use]
    pub fn of(removed: Removed, left: &[OldData]) -> Self {
        Self {
            removed: removed.removed,
            in_use: removed.in_use,
            failed: removed
                .failed
                .into_iter()
                .map(|(id, reason)| OldDataFailure { id, reason })
                .collect(),
            items: OldDataListing::of(left).items,
        }
    }
}
