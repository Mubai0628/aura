use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{PermissionSnapshotId, Revision, UtcTimestamp};
use aura_domain_common::{ToolId, ToolRegistrySnapshotId};
use aura_tool_contract::{
    EffectiveToolBindingSnapshot, ToolExposureV1, ToolLoadStateV1, ToolPinResolutionStateV1,
    ToolPinV1, ToolRegistryCatalogEntryV1, ToolRegistryErrorCode, ToolRegistryScopeV1,
    ToolRegistrySnapshotV1, ToolSchemaSnapshotV1, ToolSettingsSnapshotV1,
    ToolSourceContractFingerprintV1, ToolSourceKindV1, ToolVersion,
};
use aura_tool_domain::{
    ToolDefinitionRecordV1, ToolEphemeralLoadRecordV1, ToolInstanceRecordV1,
    ToolPackageTrustRecordV1, ToolRuntimeHealthV1,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{ToolSearchCorpusV1, embedded_builtin_tool_definition_set_v1};

#[cfg(test)]
pub(crate) mod tests_support;

mod types;
pub use types::*;
mod child;
pub use child::derive_child_registry_snapshot_v1;
#[cfg(test)]
use child::is_child_hard_denied;
mod entries;
use entries::*;
mod eligibility;
use eligibility::*;
mod evidence;
use evidence::*;
mod parent;
pub use parent::{build_parent_registry_snapshot, build_retained_parent_registry_snapshot};
