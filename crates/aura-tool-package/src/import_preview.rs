//! Pure preview assembly from authoritative staging evidence. No execution or trust.
use crate::{StagedToolPackageV1, ToolPackageStagingErrorV1};
use aura_tool_contract::{
    CommandWorkingDirectoryV1, DeclarativeCommandV1, PackageResourceClaimKeyV1,
    ToolRuntimeDescriptorV1,
};
use aura_tool_domain::{
    PackageConformanceStateV1, PackageContentDescriptorV1, PackageExecutionLocationV1,
    PackageImportPreviewV1, PackageInstallEffectV1, PackageRuntimePreviewV1,
    PackageRuntimeSupportV1, PackageVersionBindingV1, PackageWorkingDirectoryPreviewV1,
    ToolPackageManifestSchemaVersionV1,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub struct PackagePreviewDirectories<'a> {
    /// The Core-owned runtime root that installation/materialization will use.
    pub package_root: &'a Path,
    pub workspace_root: Option<&'a Path>,
    /// Core-resolved directory claims, never a default substitute for a missing claim.
    pub directory_claims: &'a BTreeMap<PackageResourceClaimKeyV1, PathBuf>,
}

#[derive(Debug, Error)]
pub enum PackageImportPreviewErrorV1 {
    #[error("package staging integrity failed")]
    Staging(#[from] ToolPackageStagingErrorV1),
    #[error("preview binding does not identify this exact package version")]
    VersionBindingMismatch,
    #[error("package runtime is not supported by import preview")]
    UnsupportedRuntime,
    #[error("preview requires an absolute, displayable directory")]
    InvalidDirectoryDisplay,
}

pub fn package_content_descriptor_v1(
    staged: &StagedToolPackageV1,
) -> Result<PackageContentDescriptorV1, ToolPackageStagingErrorV1> {
    let evidence = staged.fingerprint_evidence()?;
    Ok(PackageContentDescriptorV1 {
        manifest: staged.canonical_manifest().clone(),
        package_fingerprint: evidence.package_fingerprint().clone(),
        security_fingerprint: evidence.package_security_fingerprint().clone(),
        runtime_closure_hash: staged
            .runtime_closure()
            .canonical_hash()
            .map_err(ToolPackageStagingErrorV1::RuntimeClosureInvalid)?,
    })
}

pub fn package_import_preview_v1(
    staged: &StagedToolPackageV1,
    version: PackageVersionBindingV1,
    directories: &PackagePreviewDirectories<'_>,
    conformance: PackageConformanceStateV1,
) -> Result<PackageImportPreviewV1, PackageImportPreviewErrorV1> {
    preview(package_content_descriptor_v1(staged)?, version, Some(directories), conformance)
}

/// The preview of stored content before any run is prepared: every program working
/// directory is shown as declared, because no directory has been chosen yet.
pub fn package_content_preview_v1(
    content: PackageContentDescriptorV1,
    version: PackageVersionBindingV1,
    conformance: PackageConformanceStateV1,
) -> Result<PackageImportPreviewV1, PackageImportPreviewErrorV1> {
    preview(content, version, None, conformance)
}

fn preview(
    content: PackageContentDescriptorV1,
    version: PackageVersionBindingV1,
    directories: Option<&PackagePreviewDirectories<'_>>,
    conformance: PackageConformanceStateV1,
) -> Result<PackageImportPreviewV1, PackageImportPreviewErrorV1> {
    if version.package_fingerprint != content.package_fingerprint
        || version.package_version != content.manifest.package_version
        || version.revision.value() == 0
    {
        return Err(PackageImportPreviewErrorV1::VersionBindingMismatch);
    }
    let runtimes = content
        .manifest
        .tools
        .iter()
        .map(|tool| {
            let (directory, credential) = match tool.runtime.to_runtime_descriptor_v1() {
                ToolRuntimeDescriptorV1::DeclarativeHttp { .. } => (None, false),
                ToolRuntimeDescriptorV1::AuraStdio {
                    working_directory, credential_slot, ..
                } => (Some(working_directory), credential_slot.is_some()),
                ToolRuntimeDescriptorV1::DeclarativeCommand { command } => match command {
                    DeclarativeCommandV1::Argv { working_directory, credential_slot, .. }
                    | DeclarativeCommandV1::PowershellScript {
                        working_directory,
                        credential_slot,
                        ..
                    } => (Some(working_directory), credential_slot.is_some()),
                },
                ToolRuntimeDescriptorV1::ExternalSource { .. } => {
                    return Err(PackageImportPreviewErrorV1::UnsupportedRuntime);
                }
            };
            Ok(PackageRuntimePreviewV1 {
                tool_key: tool.tool_key.clone(),
                execution_location: if directory.is_some() {
                    PackageExecutionLocationV1::UserJob
                } else {
                    PackageExecutionLocationV1::ControlledHttp
                },
                working_directory: match (directory, directories) {
                    (None, _) => PackageWorkingDirectoryPreviewV1::NotApplicable,
                    (Some(declared), Some(directories)) => {
                        directory_preview(declared, directories)?
                    }
                    (Some(declared), None) => {
                        PackageWorkingDirectoryPreviewV1::Unresolved { declared }
                    }
                },
                support: if credential {
                    PackageRuntimeSupportV1::ProgramCredentialUnsupported
                } else {
                    PackageRuntimeSupportV1::ReadyForConformance
                },
            })
        })
        .collect::<Result<Vec<_>, PackageImportPreviewErrorV1>>()?;
    Ok(PackageImportPreviewV1 {
        contract_version: ToolPackageManifestSchemaVersionV1,
        version,
        content,
        runtimes,
        conformance,
        install_effect: PackageInstallEffectV1::TrustCurrentFingerprintAndEnable,
    })
}

fn directory_preview(
    declared: CommandWorkingDirectoryV1,
    directories: &PackagePreviewDirectories<'_>,
) -> Result<PackageWorkingDirectoryPreviewV1, PackageImportPreviewErrorV1> {
    let path = match &declared {
        CommandWorkingDirectoryV1::PackageRoot => Some(directories.package_root),
        CommandWorkingDirectoryV1::WorkspaceRoot => directories.workspace_root,
        CommandWorkingDirectoryV1::ResolvedDirectoryClaim { claim_key } => {
            directories.directory_claims.get(claim_key).map(PathBuf::as_path)
        }
    };
    match path {
        Some(path) => {
            let display = path
                .to_str()
                .filter(|value| {
                    !value.is_empty()
                        && value.encode_utf16().count() <= 32767
                        && !value.chars().any(char::is_control)
                })
                .filter(|_| path.is_absolute())
                .ok_or(PackageImportPreviewErrorV1::InvalidDirectoryDisplay)?;
            Ok(PackageWorkingDirectoryPreviewV1::Resolved { declared, path: display.to_owned() })
        }
        None => Ok(PackageWorkingDirectoryPreviewV1::Unresolved { declared }),
    }
}
