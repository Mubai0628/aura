//! V2 tool-package route contracts. Main owns file dialogs; Core owns every
//! authority check, transaction and process. Renderer values are requests only.
use crate::{
    PackageConformanceReportV1, PackageConformanceStateV1, PackageExecutionLocationV1,
    PackageImportDiagnosticV1, PackageImportPreviewV1, PackageToolSettingsV1,
    PackageVersionBindingV1, ToolCredentialManagementTargetV1, ToolCredentialPlacementV1,
};
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    CanonicalSemVer, CredentialId, CredentialRevision, Revision, ToolConformanceRunId, ToolId,
    ToolInstanceId, ToolPackageId, UtcTimestamp,
};
use aura_tool_contract::{
    CredentialSlotKey, PackageFingerprintV1, PackageKey, ToolCanonicalName, ToolKey,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Core-surface import request. The path comes from Main's own native dialog and
/// is never accepted from the renderer.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageImportSourceV1 {
    #[schemars(length(min = 1, max = 32_767))]
    pub path: String,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageImportOutcomeV1 {
    /// The user closed Main's dialog. Nothing was read.
    Cancelled,
    Imported {
        view: Box<PackageVersionViewV1>,
    },
    Rejected {
        diagnostics: Vec<PackageImportDiagnosticV1>,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageVersionRefV1 {
    pub package_id: ToolPackageId,
    pub package_version: CanonicalSemVer,
    pub package_fingerprint: PackageFingerprintV1,
}

/// One imported version as stored, with its last Conformance run.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageVersionViewV1 {
    pub preview: PackageImportPreviewV1,
    pub conformance: PackageConformanceViewV1,
    /// The settings the last run used, or the installed settings; empty when none.
    pub settings: Vec<PackageToolSettingsV1>,
    pub package_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformanceViewV1 {
    pub version: PackageVersionBindingV1,
    pub state: PackageConformanceStateV1,
    pub run_id: Option<ToolConformanceRunId>,
    pub started_at: Option<UtcTimestamp>,
    pub report: Option<PackageConformanceReportV1>,
    /// The value an install request must repeat. Present only for a finished run.
    pub report_hash: Option<CanonicalSha256HashV1>,
}

/// Prepares one run: the payload is restored, settings are checked, and the
/// concrete preview the user approves is returned. Nothing runs yet.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformancePrepareRequestV1 {
    pub version: PackageVersionBindingV1,
    #[serde(default)]
    pub settings: Vec<PackageToolSettingsV1>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformancePreparationV1 {
    #[schemars(length(min = 32, max = 32))]
    pub preparation_id: String,
    pub preview: PackageImportPreviewV1,
    /// Binds the user's approval to the actual run directory and settings.
    pub preview_hash: CanonicalSha256HashV1,
}

/// The user's explicit approval to run the prepared package code once.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformanceStartRequestV1 {
    #[schemars(length(min = 32, max = 32))]
    pub preparation_id: String,
    pub preview_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageConformanceCancelRequestV1 {
    pub version: PackageVersionRefV1,
    pub run_id: ToolConformanceRunId,
}

/// The user's explicit install approval: exact version, exact successful report and
/// the package revision the user saw.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageInstallRequestV1 {
    pub version: PackageVersionBindingV1,
    pub expected_package_revision: Revision,
    pub report_hash: CanonicalSha256HashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageEnableRequestV1 {
    pub package_id: ToolPackageId,
    pub expected_package_revision: Revision,
    pub enabled: bool,
}

/// Revoke trust or uninstall. The revision is the one the user saw in the impact view.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageChangeRequestV1 {
    pub package_id: ToolPackageId,
    pub expected_package_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCredentialBindRequestV1 {
    pub package_id: ToolPackageId,
    pub tool_key: ToolKey,
    pub slot_key: CredentialSlotKey,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    /// `None` when the slot has no binding yet.
    pub expected_binding_revision: Option<Revision>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCredentialClearRequestV1 {
    pub package_id: ToolPackageId,
    pub tool_key: ToolKey,
    pub slot_key: CredentialSlotKey,
    pub expected_binding_revision: Revision,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageListV1 {
    pub packages: Vec<PackageSummaryV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageSummaryV1 {
    pub package_id: ToolPackageId,
    pub package_key: PackageKey,
    #[schemars(length(min = 1, max = 120))]
    pub display_name: String,
    pub revision: Revision,
    pub installed: Option<PackageInstalledV1>,
    /// Newest import first. Content of other versions stays until it is collected.
    pub versions: Vec<PackageVersionSummaryV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageVersionSummaryV1 {
    pub version: PackageVersionBindingV1,
    pub conformance: PackageConformanceStateV1,
    pub imported_at: UtcTimestamp,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageInstalledV1 {
    pub version: PackageVersionRefV1,
    pub enabled: bool,
    pub trusted: bool,
    pub tools: Vec<PackageToolSummaryV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageToolSummaryV1 {
    pub tool_key: ToolKey,
    #[schemars(length(min = 1, max = 120))]
    pub display_name: String,
    pub canonical_name: ToolCanonicalName,
    pub execution_location: PackageExecutionLocationV1,
    pub tool_id: ToolId,
    pub tool_instance_id: ToolInstanceId,
    pub credential: Option<PackageCredentialSlotViewV1>,
}

/// One HTTP tool credential slot. The target is what `tools.credential.*` accepts for
/// this tool; the binding is the user's explicit choice of credential.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCredentialSlotViewV1 {
    pub slot_key: CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    /// The exact origin a bound credential's allowed websites must contain.
    pub origin: String,
    pub target: ToolCredentialManagementTargetV1,
    pub binding: Option<PackageCredentialBindingV1>,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageCredentialBindingV1 {
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub revision: Revision,
    pub bound_at: UtcTimestamp,
}
