//! Admin-published fixed resource configuration. Never accepts paths, masks,
//! credentials, network settings or commands from a caller.
use aura_domain_common::{ContentHash, UtcTimestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
pub const MAINTENANCE_VERSION: &str = "execution-maintenance-v1";
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceConfig {
    pub schema: String,
    pub installation_id: String,
    pub owner_sid: String,
    pub offline_sid: String,
    pub online_sid: Option<String>,
    /// Exact existing ACL ownership handed off during elevated Setup.
    pub initial_resources: Vec<MaintenanceResourceReceipt>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MaintenanceTarget {
    Null,
    Ksec,
    OwnerNamespace { session_id: u32, logon_id: String },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceResourceState {
    Pending,
    Applied,
    Removing,
    Removed,
    Retired,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceResourceReceipt {
    pub boot_id: String,
    pub target: MaintenanceTarget,
    pub parent_dacl: Option<String>,
    pub before: String,
    pub after: String,
    pub state: MaintenanceResourceState,
    /// Kernel object identity that survives NT BootIdentifier reuse across
    /// Restart. Absent on journals written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_generation: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MaintenanceJournal {
    pub schema: String,
    pub config_sha256: ContentHash,
    pub resources: Vec<MaintenanceResourceReceipt>,
    pub observed_at: UtcTimestamp,
    pub error: Option<String>,
}
