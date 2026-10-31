use super::{PackageRunOptions, owner::locked, types::RunState};
use crate::package_program::{
    PackageConformanceRequest, ProgramContext, ProgramInvocationIdentity,
};
use anyhow::Result;
use aura_domain_common::{
    ToolCallId, ToolConformanceRunId, ToolExecutionAttemptId, UtcTimestamp,
    jcs::CanonicalSha256HashV1,
};
use aura_persistence_v2::{V2Store, packages};
use aura_tool_domain::{
    CanonicalToolPackageManifestV1, CanonicalToolPackageRuntimeV1, PackageConformanceCaseResultV1,
    PackageConformanceReasonV1 as Reason, PackageConformanceReportV1, PackageExecutionLocationV1,
    PackageVersionBindingV1,
};
use aura_tool_package::StagedToolPackageV1;
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

pub(super) struct Work {
    pub store: Arc<std::sync::Mutex<V2Store>>,
    pub lease: Arc<super::recovery::OwnerLease>,
    pub scratch: tempfile::TempDir,
    pub staged: StagedToolPackageV1,
    pub state: Arc<RunState>,
    pub version: PackageVersionBindingV1,
    pub run_id: ToolConformanceRunId,
    pub started_at: UtcTimestamp,
    pub options: PackageRunOptions,
}

pub(super) fn spawn(work: Work) -> Result<()> {
    let store = work.store.clone();
    let state = work.state.clone();
    let fallback = failed_report(&work, Reason::CleanupUnconfirmed);
    if let Err(error) =
        std::thread::Builder::new().name("package-conformance".into()).spawn(move || execute(work))
    {
        // Spawn failed before any Job began. Dropped temporary directories may
        // still need recovery, so this is deliberately not a clean Failed run.
        let _ = finish(&store, &fallback);
        if let Ok(mut status) = state.status.lock() {
            status.running = false;
            status.failure_code = Some("package.worker_not_started");
        }
        return Err(error.into());
    }
    Ok(())
}

fn execute(work: Work) {
    let fallback = failed_report(&work, Reason::CleanupUnconfirmed);
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let context = ProgramContext {
            identity: ProgramInvocationIdentity {
                tool_call_id: ToolCallId::new(),
                tool_execution_attempt_id: ToolExecutionAttemptId::new(),
            },
            workspace_root: work.options.workspace_root.as_deref(),
            directory_claims: work.options.directory_claims.clone(),
            instance_configuration: None,
            powershell: work.options.powershell.as_deref(),
            user_environment: work.options.user_environment.clone(),
            scratch_parent: work.scratch.path(),
            timeout: work.options.timeout,
            max_output_bytes: work.options.max_output_bytes,
        };
        crate::package_program::run_conformance(
            &work.staged,
            PackageConformanceRequest {
                version: work.version.clone(),
                run_id: work.run_id,
                started_at: work.started_at,
                settings: work.options.settings.clone(),
            },
            &context,
            &work.state.cancelled,
            &mut |_, _, pid| {
                if let Ok(mut status) = work.state.status.lock() {
                    status.process_id = Some(pid);
                }
            },
        )
    }));
    let mut report = match outcome {
        Ok(Ok((report, _))) => report,
        Ok(Err(_)) | Err(_) => fallback,
    };
    // The run owns both materialized payload and call roots. Explicitly observe
    // cleanup after the runtime has stopped; TempDir's silent Drop is not proof.
    drop(work.staged);
    if work.scratch.close().is_err() {
        report.cleanup_verified = false;
    }
    report.completed_at = UtcTimestamp::now();
    let persisted = finish(&work.store, &report);
    // Content an uninstall left to this run is collected once the run lets it go.
    if persisted.is_ok() {
        let _ = locked(&work.store).map(|mut store| {
            store.database.unit_of_work(|tx| {
                packages::lifecycle::collect_unreferenced(tx, UtcTimestamp::now())
            })
        });
    }
    // Keep the lease until both Job/cleanup and durable settlement have ended.
    drop(work.lease);
    if let Ok(mut status) = work.state.status.lock() {
        status.running = false;
        status.process_id = None;
        if persisted.is_err() {
            status.failure_code = Some("package.conformance_commit_unconfirmed");
        }
    }
}

fn finish(store: &std::sync::Mutex<V2Store>, report: &PackageConformanceReportV1) -> Result<()> {
    Ok(locked(store)?
        .database
        .unit_of_work(|tx| packages::conformance::finish_conformance(tx, report))?)
}

fn failed_report(work: &Work, reason: Reason) -> PackageConformanceReportV1 {
    PackageConformanceReportV1 {
        run_id: work.run_id,
        version: work.version.clone(),
        started_at: work.started_at,
        completed_at: UtcTimestamp::now(),
        cases: failed_cases(work.staged.canonical_manifest(), reason),
        cleanup_verified: false,
        settings_hash: aura_tool_domain::package_settings_hash_v1(&work.options.settings)
            .unwrap_or_else(|_| CanonicalSha256HashV1::sha256(b"settings hash unavailable")),
    }
}

fn failed_cases(
    manifest: &CanonicalToolPackageManifestV1,
    reason: Reason,
) -> Vec<PackageConformanceCaseResultV1> {
    manifest
        .tools
        .iter()
        .flat_map(|tool| {
            tool.conformance_cases.iter().map(move |case| PackageConformanceCaseResultV1 {
                tool_key: tool.tool_key.clone(),
                case_name: case.name.clone(),
                reason,
                execution_location: if matches!(
                    tool.runtime,
                    CanonicalToolPackageRuntimeV1::DeclarativeHttp { .. }
                ) {
                    PackageExecutionLocationV1::ControlledHttp
                } else {
                    PackageExecutionLocationV1::UserJob
                },
                runtime_evidence_hash: CanonicalSha256HashV1::sha256(
                    b"Core could not confirm the Conformance worker outcome",
                ),
            })
        })
        .collect()
}
