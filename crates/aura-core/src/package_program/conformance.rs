use super::{ProgramContext, ProgramInvocationIdentity, run_case};
use anyhow::{Result, ensure};
use aura_domain_common::{ToolConformanceRunId, UtcTimestamp, jcs::CanonicalSha256HashV1};
use aura_tool_domain::{
    CanonicalToolPackageRuntimeV1, PackageConformanceCaseResultV1,
    PackageConformanceReasonV1 as Reason, PackageConformanceReportV1, PackageExecutionLocationV1,
    PackageToolSettingsV1, PackageVersionBindingV1, canonical_package_settings_v1,
    package_settings_hash_v1,
};
use aura_tool_package::StagedToolPackageV1;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct PackageConformanceRequest {
    pub version: PackageVersionBindingV1,
    pub run_id: ToolConformanceRunId,
    pub started_at: UtcTimestamp,
    /// Canonical per-tool settings; the report binds their hash.
    pub settings: Vec<PackageToolSettingsV1>,
}

/// Keep bounded observations, not every case's stdout/artifact bytes. Outputs
/// have already been checked and hashed before the next case may run.
pub struct ProgramCaseObservation {
    pub identity: ProgramInvocationIdentity,
    pub started: bool,
    pub stop_confirmed: bool,
}

/// No trust/enable mutation occurs here. The caller must record the exact run
/// before invoking this and require a separate authenticated install approval.
pub fn run_conformance(
    staged: &StagedToolPackageV1,
    request: PackageConformanceRequest,
    context: &ProgramContext<'_>,
    cancelled: &AtomicBool,
    on_started: &mut dyn FnMut(
        &aura_tool_contract::ToolKey,
        &aura_tool_domain::ToolPackageConformanceCaseNameV1,
        u32,
    ),
) -> Result<(PackageConformanceReportV1, Vec<ProgramCaseObservation>)> {
    let PackageConformanceRequest { version, run_id, started_at, settings } = request;
    let content = aura_tool_package::package_content_descriptor_v1(staged)?;
    ensure!(
        content.package_fingerprint == version.package_fingerprint
            && content.manifest.package_version == version.package_version,
        "package.conformance_version_changed"
    );
    ensure!(
        canonical_package_settings_v1(&content.manifest, &settings)? == settings,
        "package.settings_not_canonical"
    );
    let settings_hash = package_settings_hash_v1(&settings)?;
    let settings_for =
        |key: &aura_tool_contract::ToolKey| settings.iter().find(|entry| &entry.tool_key == key);
    let mut cases = Vec::new();
    let mut runs = Vec::new();
    let mut cleanup_verified = true;
    let mut blocked = false;
    for tool in &staged.canonical_manifest().tools {
        for case in &tool.conformance_cases {
            let http =
                matches!(tool.runtime, CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. });
            let (reason, hash) = if blocked {
                (
                    Reason::CleanupUnconfirmed,
                    CanonicalSha256HashV1::sha256(b"previous invocation is not fully settled"),
                )
            } else if cancelled.load(Ordering::Acquire) {
                (Reason::Cancelled, CanonicalSha256HashV1::sha256(b"cancelled before dispatch"))
            } else if http {
                crate::package_http::run_case(
                    tool,
                    case.arguments.value(),
                    settings_for(&tool.tool_key),
                )
            } else {
                let claims = super::claims::scratch_directory_claims(
                    tool,
                    case.arguments.value(),
                    context.workspace_root,
                );
                let context = ProgramContext {
                    identity: ProgramInvocationIdentity {
                        tool_call_id: aura_domain_common::ToolCallId::new(),
                        tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId::new(
                        ),
                    },
                    instance_configuration: settings_for(&tool.tool_key)
                        .and_then(|entry| entry.configuration.as_ref()),
                    directory_claims: claims.as_ref().cloned().unwrap_or_default(),
                    ..context.clone()
                };
                if claims.is_err() {
                    (
                        Reason::ProcessFailed,
                        CanonicalSha256HashV1::sha256(
                            b"the declared working directory is unavailable",
                        ),
                    )
                } else {
                    match run_case(
                        staged,
                        &tool.tool_key,
                        case.arguments.value(),
                        &context,
                        cancelled,
                        &mut |pid| on_started(&tool.tool_key, &case.name, pid),
                    ) {
                        Ok(run) => {
                            cleanup_verified &=
                                run.stop_confirmed && run.reason != Reason::CleanupUnconfirmed;
                            blocked = !cleanup_verified;
                            let value = (run.reason, run.runtime_evidence_hash.clone());
                            runs.push(ProgramCaseObservation {
                                identity: run.identity,
                                started: run.started,
                                stop_confirmed: run.stop_confirmed,
                            });
                            value
                        }
                        Err(error) => {
                            let reason = if error.to_string()
                                == "package.execution_report_unconfirmed"
                                || error
                                    .chain()
                                    .any(|cause| cause.to_string() == "package.cleanup_unconfirmed")
                            {
                                cleanup_verified = false;
                                blocked = true;
                                Reason::CleanupUnconfirmed
                            } else if error.chain().any(|cause| {
                                cause.to_string() == "package.program_credentials_unsupported"
                            }) {
                                Reason::ProgramCredentialUnsupported
                            } else {
                                Reason::ProcessFailed
                            };
                            (
                                reason,
                                CanonicalSha256HashV1::sha256(
                                    b"package runtime did not produce a validated report",
                                ),
                            )
                        }
                    }
                }
            };
            cases.push(PackageConformanceCaseResultV1 {
                tool_key: tool.tool_key.clone(),
                case_name: case.name.clone(),
                execution_location: if http {
                    PackageExecutionLocationV1::ControlledHttp
                } else {
                    PackageExecutionLocationV1::UserJob
                },
                reason,
                runtime_evidence_hash: hash,
            });
        }
    }
    Ok((
        PackageConformanceReportV1 {
            run_id,
            version,
            started_at,
            completed_at: UtcTimestamp::now(),
            cases,
            cleanup_verified,
            settings_hash,
        },
        runs,
    ))
}
