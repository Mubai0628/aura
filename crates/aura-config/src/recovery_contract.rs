//! Renderer-safe recovery boundary. Mechanisms live in service/recovery, composition in Core.
use crate::{
    ConfigModule,
    recovery::{ConfigRecoveryAction, RecoveryOutcome},
};

#[derive(
    Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RecoveryNotice {
    /// The configuration domains whose fault is being reported, by key.
    pub affected_domains: Vec<String>,
    /// The capabilities that are unavailable because of those faults, by key.
    pub unavailable_capabilities: Vec<String>,
    /// Stable codes for each fault. What the user's report will quote.
    pub fault_codes: Vec<String>,
    /// New calls that are suspended, described in bounded terms.
    pub suspended_calls: Vec<String>,
    /// The actions the user may take, by key.
    pub available_actions: Vec<String>,
    /// Whether the runtime can say which domains are affected.
    ///
    /// `false` is the honest "unknown scope": the user is told the runtime cannot
    /// narrow it, rather than being shown a misleadingly precise list.
    pub scope_known: bool,
    /// A recovery is still owned by this runtime; a state read is not its receipt.
    pub recovery_in_progress: bool,
    /// A bounded next step for the user.
    pub next_step: String,
}

#[derive(
    Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct RecoveryReport {
    pub action: ConfigRecoveryAction,
    pub outcomes: Vec<RecoveryOutcome>,
    /// The capabilities whose service is **really present** after this attempt.
    pub restored_capabilities: Vec<String>,
    /// The capabilities still unavailable afterwards, each with its stable reason.
    ///
    /// The previous report had nowhere to put this, so a
    /// capability whose construction genuinely failed was indistinguishable from one that came
    /// back. The user saw a cleared fault marker and concluded the feature worked.
    #[serde(default)]
    pub unavailable_capabilities: Vec<UnavailableCapability>,
    /// A stable code when the attempt did not fully apply.
    pub reason_code: Option<String>,
}

#[derive(
    Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct UnavailableCapability {
    /// The capability, by stable key.
    pub capability: String,
    /// Why it is still unavailable, in bounded, path-free terms.
    pub reason: String,
}

#[derive(
    Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ConfigRecoveryPlanRequest {
    #[serde(default)]
    pub domains: Vec<ConfigModule>,
}

#[derive(
    Clone, Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ConfigRecoveryApplyRequest {
    /// The action the user chose, from the previewed options.
    pub action: ConfigRecoveryAction,
    /// A single domain, for a single-domain recovery.
    #[serde(default)]
    pub domain: Option<ConfigModule>,
    /// Several domains, for a cross-domain recovery that must reach one version.
    #[serde(default)]
    pub domains: Vec<ConfigModule>,
    /// The identity of the preview the user confirmed.
    ///
    /// The apply used to **re-plan** from the
    /// request's domains and discard the preview entirely, so the plan the user was
    /// shown was never the plan applied, and a stale confirmation could overwrite
    /// newer valid configuration. Carrying the previewed identity across the wire is
    /// what lets the write refuse a confirmation that no longer describes the live
    /// state.
    ///
    /// It is **required**. It used to be an `Option`, and
    /// `None` meant "apply the current state", which was a bypass: a caller that omitted the
    /// field wrote configuration no user had confirmed. A missing identity is now refused
    /// before any write, exactly like a stale one.
    pub preview_id: String,
}
