use super::*;
use crate::controller::budgets;
use aura_backend_api::ExecutionBackend;
use aura_domain_common::{RuntimeInstanceId, UtcTimestamp};
use aura_execution_contract::{
    AttemptRef, BackendReadiness, DispatchRequest, ReconcileResult, SecuredOperation,
    TerminationEvidence,
};
use aura_security_contract::PermissionMode;
use aura_windows_user_contract::{RunnerBackgroundUpdate, RunnerObservation, RunnerProgress};
use std::sync::Arc;

struct UnusedPort;

impl WindowsUserCorePort for UnusedPort {
    fn begin_launch<'a>(&'a self, _: &'a DispatchRequest) -> LaunchFuture<'a> {
        unreachable!("descriptor must not launch")
    }
    fn scope(&self, _: &SecuredOperation) -> anyhow::Result<ScopeFacts> {
        unreachable!("descriptor must not request operation scope")
    }

    fn claim(
        &self,
        _: &DispatchRequest,
        _: &mut dyn FnMut() -> anyhow::Result<()>,
    ) -> anyhow::Result<UtcTimestamp> {
        unreachable!("descriptor must not claim an execution")
    }

    fn started(&self, _: &DispatchRequest, _: u32, _: UtcTimestamp) -> anyhow::Result<()> {
        unreachable!("descriptor must not start an execution")
    }

    fn progress(&self, _: &DispatchRequest, _: &RunnerProgress) -> anyhow::Result<()> {
        unreachable!("descriptor must not report progress")
    }

    fn diagnostic(&self, _: &DispatchRequest, _: &str) -> anyhow::Result<()> {
        unreachable!("descriptor must not report diagnostics")
    }

    fn background(&self, _: &DispatchRequest, _: &RunnerBackgroundUpdate) -> anyhow::Result<()> {
        unreachable!("descriptor must not report background state")
    }

    fn runtime_stopped(
        &self,
        _: &DispatchRequest,
        _: bool,
        _: bool,
    ) -> anyhow::Result<TerminationEvidence> {
        unreachable!("descriptor must not stop a runtime")
    }

    fn complete(
        &self,
        _: &DispatchRequest,
        _: Option<&RunnerObservation>,
        _: bool,
        _: bool,
        _: Option<NotStarted>,
        _: Option<&str>,
    ) -> anyhow::Result<TerminationEvidence> {
        unreachable!("descriptor must not complete an execution")
    }

    fn reconcile(&self, _: AttemptRef) -> anyhow::Result<ReconcileResult> {
        unreachable!("descriptor must not reconcile an execution")
    }
}

fn backend(root: std::path::PathBuf) -> WindowsUserBackend {
    WindowsUserBackend::new(
        Arc::new(installation::InstallationBinding::new(root)),
        PermissionMode::WorkspaceReadWrite,
        RuntimeInstanceId::new(),
        Arc::new(UnusedPort),
        Arc::new(tokio::sync::Semaphore::new(1)),
    )
}

#[test]
fn stable_registration_id_does_not_open_installation() {
    let root = std::path::PathBuf::from(r"Z:\aura-registration-must-not-read");
    let backend = backend(root);
    assert_eq!(backend.backend_id().0, "windows-user-workspace-v1");
}

#[test]
fn damaged_published_installation_requires_repair() {
    let root = tempfile::tempdir().expect("temporary installation");
    std::fs::write(root.path().join("execution.json"), b"{}").expect("damaged manifest");
    let descriptor = backend(root.path().to_path_buf()).descriptor();
    assert!(matches!(
        descriptor.readiness,
        BackendReadiness::RepairRequired { ref reason }
            if reason == "runner.installation_or_resources_changed"
    ));
}

#[test]
fn missing_installation_requires_setup() {
    let root = tempfile::tempdir().expect("temporary installation");
    let backend = backend(root.path().to_path_buf());
    let first = backend.descriptor();
    let second = backend.descriptor();
    assert!(matches!(
        first.readiness,
        BackendReadiness::SetupRequired { ref reason }
            if reason == "runner.production_installation_required"
    ));
    assert_eq!(first.readiness, second.readiness);
}

/// A recorded launch sub-stage is one stage name and one bounded duration.
///
/// This is the shape a reader of the attempt journal depends on: adding the stages between
/// Core's prepare and the script process starting has to be possible without guessing a
/// field name or filtering an unbounded payload. `LaunchStage` carries nothing else — no
/// path, no script, no nonce — so recording every stage of every launch stays bounded.
///
/// A port that measures nothing needs no method: `UnusedPort` above implements no timing
/// call at all, and the launch path still compiles against it. That is what keeps "this port
/// records no timings" expressible without inventing zeroes for stages nobody measured.
#[test]
fn a_launch_stage_record_carries_a_stage_name_and_one_duration() {
    for (stage, name) in [
        (LaunchStageKind::Verified, "verified"),
        (LaunchStageKind::ResourcesAllocated, "resources_allocated"),
        (LaunchStageKind::PipeCreated, "pipe_created"),
        (LaunchStageKind::ProcessCreated, "process_created"),
        (LaunchStageKind::PipeConnected, "pipe_connected"),
        (LaunchStageKind::RunnerReady, "runner_ready"),
        (LaunchStageKind::WorkspaceGranted, "workspace_granted"),
        (LaunchStageKind::LaunchClaimed, "launch_claimed"),
        (LaunchStageKind::Dispatched, "dispatched"),
    ] {
        let wire = serde_json::to_value(LaunchStage { stage, ms: 1_234 }).expect("wire");
        assert_eq!(wire, serde_json::json!({ "stage": name, "ms": 1_234 }));
    }
}

/// A recorded launch work record is bounded counts and one flag.
///
/// The stage timings say where a launch spent its time; this record says why the
/// expensive stage was expensive. It is the reason a reader can tell "the ACL walk
/// touched 14,000 objects" from "the Runner handshake waited", without either number
/// being inferred from wall-clock differences.
///
/// The promise that matters is what is *absent*: no path, no script text, no credential,
/// no nonce, no directory content. `UnusedPort` implements no counting call at all and
/// the launch path still compiles, so a port that measures nothing does not have to
/// publish zeroes for work nobody counted.
#[test]
fn a_launch_work_record_carries_bounded_counts_only() {
    let work = crate::LaunchWorkV1 {
        roots_granted: 2,
        objects_scanned: 14_001,
        directories_scanned: 812,
        acl_rewrites: 13_998,
        objects_skipped: 3,
        journal_records: 14_001,
        ancestor_roots_granted: 2,
        ancestors_pinned: 9,
        ancestor_acl_rewrites: 2,
        installation_checks: 3,
        runtime_reused: true,
        active_resources: 1,
        background_resources: 0,
        hashed_bytes: 512,
        recovery_ms: 7,
        recovery_journals: 3,
        recovery_bytes: 128_000,
        recovery_records: 320,
        recovery_indexed: 2,
        recovery_index_checks: 2,
        queue_ms: 4,
        preparation_ms: 30_558,
        startup_ms: 210,
        launch_window_ms: 29_790,
    };
    let wire = serde_json::to_value(work).expect("wire");
    let object = wire.as_object().expect("object");
    assert_eq!(object.len(), 24, "the record explains the launch and nothing else");
    for (key, value) in object {
        assert!(
            value.is_u64() || value.is_boolean(),
            "`{key}` must be a count or a flag, never {value}"
        );
    }
    assert_eq!(object["objects_scanned"], serde_json::json!(14_001));
    assert_eq!(object["acl_rewrites"], serde_json::json!(13_998));
    assert_eq!(object["runtime_reused"], serde_json::json!(true));
    assert_eq!(object["hashed_bytes"], serde_json::json!(512));
    assert_eq!(object["recovery_journals"], serde_json::json!(3));
    assert_eq!(object["recovery_indexed"], serde_json::json!(2));
    assert_eq!(object["recovery_index_checks"], serde_json::json!(2));
    // The shape the incident produced: a long preparation and a launch window that
    // was still whole when Execute went out, instead of a window already spent.
    assert_eq!(object["preparation_ms"], serde_json::json!(30_558));
    assert_eq!(object["launch_window_ms"], serde_json::json!(29_790));
}

/// The defect this batch exists for: a long preparation must not consume the window.
///
/// Both CMD launches in the incident spent 30.6 s and 32.9 s in access-control work
/// against a thirty-second window that had been issued *before* that work started, so
/// they were refused for having prepared. The window is now decided after the work, so
/// the same preparation leaves the whole window. Raising the number would have hidden
/// this instead of fixing it, which is why the assertions below are about *when* the
/// window starts rather than how long it is.
#[test]
fn a_preparation_longer_than_the_window_still_leaves_a_fresh_launch_window() {
    let started = UtcTimestamp::now();
    // The preparation that failed: thirty-one to forty seconds of ACL work.
    let prepared_for_seconds = 40;
    let now =
        UtcTimestamp::from_value(started.value() + time::Duration::seconds(prepared_for_seconds));
    let authority_deadline =
        UtcTimestamp::from_value(started.value() + time::Duration::seconds(5 * 60));
    let plan_valid_until = UtcTimestamp::from_value(
        started.value()
            + time::Duration::seconds(
                i64::try_from(budgets::PREPARATION_TRUST.as_secs()).unwrap_or(i64::MAX),
            ),
    );
    let window = crate::supervisor::launch_window(now, authority_deadline, plan_valid_until)
        .expect("a finished preparation must still have a launch window");
    let remaining_ms = (window.value() - now.value()).whole_milliseconds();
    assert!(
        remaining_ms >= 29_000,
        "the window must be whole at the moment it is issued, not the remainder of a \
         preparation budget: {remaining_ms} ms left"
    );
    assert!(
        window <= authority_deadline,
        "a fresh window may never outlive the authority it belongs to"
    );
}

/// The window is still bounded by both ends that matter.
#[test]
fn the_launch_window_never_outlives_the_authority_or_the_plan() {
    let now = UtcTimestamp::now();
    let plan_valid_until = UtcTimestamp::from_value(now.value() + time::Duration::seconds(10 * 60));

    // An authority with less time left than the window: the authority wins.
    let short_authority = UtcTimestamp::from_value(now.value() + time::Duration::seconds(5));
    let window = crate::supervisor::launch_window(now, short_authority, plan_valid_until)
        .expect("five seconds is a window");
    assert_eq!(window, short_authority, "the authority's own deadline is the outer bound");

    // A plan with less time left than the window: the plan wins.
    let short_plan = UtcTimestamp::from_value(now.value() + time::Duration::seconds(7));
    let far_authority = UtcTimestamp::from_value(now.value() + time::Duration::seconds(10 * 60));
    let window = crate::supervisor::launch_window(now, far_authority, short_plan)
        .expect("seven seconds is a window");
    assert_eq!(window, short_plan, "the plan bound is the other outer bound");

    // Neither: there is no window, and that is a refusal before anything is created.
    let expired_authority = UtcTimestamp::from_value(now.value() - time::Duration::seconds(1));
    assert_eq!(crate::supervisor::launch_window(now, expired_authority, plan_valid_until), None);
    let expired_plan = UtcTimestamp::from_value(now.value() - time::Duration::seconds(1));
    assert_eq!(crate::supervisor::launch_window(now, far_authority, expired_plan), None);
}
