//! V2 package import/preview contracts. An import is never installation or trust.
use aura_domain_common::{CanonicalSemVer, Revision, ToolPackageId};
use aura_tool_contract::{
    CommandWorkingDirectoryV1, PackageFingerprintV1, PackageRuntimeClosureHashV1,
    PackageSecurityFingerprintV1, ToolKey,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{CanonicalToolPackageManifestV1, ToolPackageManifestSchemaVersionV1};

/// Immutable content facts; directory bindings and Conformance are deliberately
/// outside this object so identical content can share one payload.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageContentDescriptorV1 {
    pub manifest: CanonicalToolPackageManifestV1,
    pub package_fingerprint: PackageFingerprintV1,
    pub security_fingerprint: PackageSecurityFingerprintV1,
    pub runtime_closure_hash: PackageRuntimeClosureHashV1,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageVersionBindingV1 {
    pub package_id: ToolPackageId,
    pub package_version: CanonicalSemVer,
    pub package_fingerprint: PackageFingerprintV1,
    pub revision: Revision,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageConformanceStateV1 {
    NotRun,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageExecutionLocationV1 {
    ControlledHttp,
    /// The author's code runs with the user's identity, with Job/output controls.
    UserJob,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageWorkingDirectoryPreviewV1 {
    NotApplicable,
    Resolved {
        declared: CommandWorkingDirectoryV1,
        /// Absolute OS directory for display; it grants no resource authority.
        #[schemars(length(min = 1, max = 32_767))]
        path: String,
    },
    /// Preview may precede workspace/resource selection. No directory is
    /// substituted; execution must obtain a resolved binding first.
    Unresolved {
        declared: CommandWorkingDirectoryV1,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageRuntimeSupportV1 {
    ReadyForConformance,
    /// A program tool declared a credential slot. Program tools run with the user's
    /// identity, and the Native exception never covers credential isolation, so the
    /// package cannot be installed.
    ProgramCredentialUnsupported,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageRuntimePreviewV1 {
    pub tool_key: ToolKey,
    pub execution_location: PackageExecutionLocationV1,
    pub working_directory: PackageWorkingDirectoryPreviewV1,
    pub support: PackageRuntimeSupportV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageInstallEffectV1 {
    TrustCurrentFingerprintAndEnable,
}

/// The full canonical manifest retains tool descriptions, capabilities, resource
/// claims, HTTP target/path mappings and credential slot declarations. UI must
/// display these rather than infer an endpoint or omit a declared requirement.
#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageImportPreviewV1 {
    pub contract_version: ToolPackageManifestSchemaVersionV1,
    pub version: PackageVersionBindingV1,
    pub content: PackageContentDescriptorV1,
    pub runtimes: Vec<PackageRuntimePreviewV1>,
    pub conformance: PackageConformanceStateV1,
    pub install_effect: PackageInstallEffectV1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PackageImportDiagnosticCodeV1 {
    InvalidManifest,
    InvalidSource,
    PathDenied,
    LimitExceeded,
    IntegrityMismatch,
    StorageUnavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PackageImportDiagnosticLocationV1 {
    Manifest { line: Option<u32>, column: Option<u32>, field: Option<String> },
    PackageFiles,
    ImportSource,
    Staging,
}

/// Fixed, user-facing reasons. Never includes Rust type names, raw JSON values,
/// credentials or a chained operating-system error.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackageImportDiagnosticV1 {
    pub code: PackageImportDiagnosticCodeV1,
    pub location: PackageImportDiagnosticLocationV1,
    pub message: String,
}
