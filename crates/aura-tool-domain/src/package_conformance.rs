//! V2 Conformance facts bind one exact import revision, never a legacy Draft.
use crate::{PackageConformanceStateV1, PackageExecutionLocationV1, PackageVersionBindingV1};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ToolConformanceRunId, UtcTimestamp};
use aura_tool_contract::{ToolKey, ToolPackageConformanceCaseNameV1};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageConformanceReasonV1 {
    Completed,
    InvalidResult,
    InvalidProtocol,
    InvalidArtifact,
    Cancelled,
    TimedOut,
    OutputLimit,
    ProcessFailed,
    TransportUnconfirmed,
    CleanupUnconfirmed,
    CredentialReflected,
    /// A program tool declared a credential slot. The Native exception never covers
    /// credential isolation, so such a tool cannot use Aura-held credentials.
    ProgramCredentialUnsupported,
    /// The request and status were checked, but no minimal instance of the declared
    /// structured result could be synthesized; the first real call validates it.
    CompletedResultUnchecked,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformanceCaseResultV1 {
    pub tool_key: ToolKey,
    pub case_name: ToolPackageConformanceCaseNameV1,
    pub execution_location: PackageExecutionLocationV1,
    pub reason: PackageConformanceReasonV1,
    pub runtime_evidence_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformanceReportV1 {
    pub run_id: ToolConformanceRunId,
    pub version: PackageVersionBindingV1,
    pub started_at: UtcTimestamp,
    pub completed_at: UtcTimestamp,
    pub cases: Vec<PackageConformanceCaseResultV1>,
    pub cleanup_verified: bool,
    /// The exact settings every case ran with; install binds this through the report hash.
    pub settings_hash: CanonicalSha256HashV1,
}

impl PackageConformanceReportV1 {
    #[must_use]
    pub fn state(&self) -> PackageConformanceStateV1 {
        if !self.cleanup_verified
            || self.cases.iter().any(|case| {
                matches!(
                    case.reason,
                    PackageConformanceReasonV1::CleanupUnconfirmed
                        | PackageConformanceReasonV1::TransportUnconfirmed
                )
            })
        {
            PackageConformanceStateV1::Interrupted
        } else if self.cases.iter().any(|case| case.reason == PackageConformanceReasonV1::Cancelled)
        {
            PackageConformanceStateV1::Cancelled
        } else if !self.cases.is_empty() && self.cases.iter().all(|case| case.reason.completed()) {
            PackageConformanceStateV1::Succeeded
        } else {
            PackageConformanceStateV1::Failed
        }
    }
}

impl PackageConformanceReasonV1 {
    /// Completed cases. `CompletedResultUnchecked` ran and passed every check it could make.
    #[must_use]
    pub const fn completed(self) -> bool {
        matches!(self, Self::Completed | Self::CompletedResultUnchecked)
    }
}
