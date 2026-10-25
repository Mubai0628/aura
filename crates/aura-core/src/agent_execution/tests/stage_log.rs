//! Stage diagnostics emitted by a real `execute_call`, read back from the log.
use super::*;
use crate::CoreLogRoute;
use aura_backend_api::{BackendFuture, ExecutionBackend};
use aura_backend_windows_user::installation::InstallationBinding;
use aura_domain_common::jcs::canonical_sha256;
use aura_execution_contract::{
    AttemptRef, AuthorizedDispatch, BackendDescriptor, BackendError, CancelReceipt,
    ExecutionHandle, ExecutionOutcome, ExecutionPlan, ExecutionReceipt, ExecutionResult,
    PrepareRequest, PreparedExecution, ReconcileResult, RollbackEvidence, SideEffectStateV1,
    TerminationEvidence,
};
use fixture::Fixture;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

struct RoutedLog {
    // Declared first: the route ends before its directory is removed.
    _route: CoreLogRoute,
    _dir: tempfile::TempDir,
    path: PathBuf,
}

impl RoutedLog {
    fn open() -> Self {
        let dir = tempfile::tempdir().expect("Agent test setup and persisted assertion");
        let path = dir.path().join("core.jsonl");
        // The backend's installation records follow the route as well.
        let route = CoreLogRoute::to(path.clone());
        Self { _route: route, _dir: dir, path }
    }

    fn lines(&self) -> Vec<Value> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        text.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).collect()
    }
}

struct LyingBackend {
    descriptor: BackendDescriptor,
    installation: Arc<InstallationBinding>,
}

impl ExecutionBackend for LyingBackend {
    fn descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }

    fn prepare(&self, request: PrepareRequest) -> BackendFuture<'_, ExecutionPlan> {
        // The first open fails on an empty root. The second caller, still inside
        // this execute_call, must record only its own wait.
        let _ = self.installation.current();
        let _ = self.installation.current();
        let descriptor = self.descriptor.clone();
        Box::pin(async move {
            ExecutionPlan::new(PreparedExecution {
                operation_fingerprint: request.operation.fingerprint,
                backend: descriptor,
                preparation: serde_json::json!({}),
            })
            .map_err(|error| BackendError::PreparationRejected { reason: error.to_string() })
        })
    }

    fn execute(&self, dispatch: AuthorizedDispatch) -> BackendFuture<'_, ExecutionHandle> {
        let request = dispatch.request().clone();
        Box::pin(async move {
            let fingerprint = canonical_sha256(&request.binding).map_err(|_| {
                BackendError::DispatchUnknown { attempt: request.binding.attempt.clone() }
            })?;
            Ok(ExecutionHandle {
                attempt: request.binding.attempt,
                dispatch_fingerprint: fingerprint,
            })
        })
    }

    fn cancel(&self, handle: ExecutionHandle) -> BackendFuture<'_, CancelReceipt> {
        Box::pin(async move { Err(BackendError::DispatchUnknown { attempt: handle.attempt }) })
    }

    fn reconcile(&self, attempt: AttemptRef) -> BackendFuture<'_, ReconcileResult> {
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            let evidence = aura_domain_common::ArtifactId::new();
            let result = ExecutionResult {
                outcome: ExecutionOutcome::Failed,
                side_effect: SideEffectStateV1::NoEffect,
                receipt: ExecutionReceipt {
                    attempt,
                    evidence_ref: evidence,
                    postcondition_fingerprint: None,
                    rollback: RollbackEvidence::NotApplicable,
                },
                termination: TerminationEvidence::Confirmed { evidence_ref: evidence },
                completed_at: aura_domain_common::UtcTimestamp::now(),
                warnings: Vec::new(),
            };
            Ok(ReconcileResult::Completed { result: Box::new(result) })
        })
    }
}

fn stage_lines<'a>(lines: &'a [Value], call_id: &str) -> Vec<&'a Value> {
    lines
        .iter()
        .filter(|line| line["event"] == "tool.stage" && line["fields"]["tool_call_id"] == call_id)
        .collect()
}

#[tokio::test]
async fn execute_call_logs_prepare_cancel_commit_and_shared_installation_wait() {
    let log = RoutedLog::open();
    let fixture = Fixture::new(PermissionMode::WorkspaceReadWrite).await;
    let admitted = fixture.admit(&[(
        "fs.write",
        serde_json::json!({"path":"stage.txt","text":"stage","expected":{"expected_kind":"absent"}}),
    )]);
    let call_id = admitted.batch.calls[0].tool_call_id;
    let id = call_id.to_string();
    let plan = admitted.planned.execution_plan.calls[0].clone();
    let generation = admitted.planned.runtime_calls[0].generation.clone();
    let call = admitted.calls[&call_id].clone();
    let execution = &fixture.service().agent_execution_required();

    let mut drifted = call.clone();
    drifted.registry.created_at = aura_domain_common::UtcTimestamp::now();
    let real = execution.broker_backend(&call).expect("Agent test setup and persisted assertion");
    let (_keep, cancel) = tokio::sync::watch::channel(false);
    let prepared = execution
        .execute_call(drifted, &plan, generation.clone(), std::slice::from_ref(&real), cancel)
        .await;
    let Err(prepared_error) = prepared else {
        panic!("a drifted registry fails before a backend exists");
    };
    assert!(prepared_error.to_string().contains("Registry Snapshot"), "{prepared_error}");
    let prepared_log = log.lines();
    let prepared_lines = stage_lines(&prepared_log, &id);
    assert_eq!(prepared_lines.len(), 1);
    let fields = &prepared_lines[0]["fields"];
    assert_eq!(fields["tool_batch_id"], call.batch.tool_batch_id.to_string());
    assert_eq!(fields["agent_run_id"], call.batch.invocation_identity.agent_run_id.to_string());
    assert_eq!(fields["outcome"], "error");
    assert_eq!(fields["wait_ms"], 0);
    assert_eq!(fields["commit_ms"], 0);
    assert!(fields["prepare_ms"].as_u64().is_some());

    let (_keep, cancel) = tokio::sync::watch::channel(true);
    let cancelled =
        execution.execute_call(call.clone(), &plan, generation.clone(), &[real], cancel).await;
    let Err(cancelled_error) = cancelled else {
        panic!("an already cancelled call stops in dispatch");
    };
    assert!(cancelled_error.to_string().contains("cancelled"), "{cancelled_error}");
    let cancelled_log = log.lines();
    let cancelled_lines = stage_lines(&cancelled_log, &id);
    assert_eq!(cancelled_lines.len(), 2);
    let fields = &cancelled_lines[1]["fields"];
    assert_eq!(fields["outcome"], "error");
    assert_eq!(fields["execute_ms"], 0);
    assert_eq!(fields["commit_ms"], 0);
    assert!(fields["wait_ms"].as_u64().is_some());
    assert_eq!(fields["tool_call_id"], id);

    let root = tempfile::tempdir().expect("Agent test setup and persisted assertion");
    let installation = Arc::new(InstallationBinding::new(absolute_empty(root.path())));
    let lying: Arc<dyn ExecutionBackend> = Arc::new(LyingBackend {
        descriptor: execution
            .broker_backend(&call)
            .expect("Agent test setup and persisted assertion")
            .descriptor(),
        installation,
    });
    let (_keep, cancel) = tokio::sync::watch::channel(false);
    let committed =
        execution.execute_call(call, &plan, generation, std::slice::from_ref(&lying), cancel).await;
    let Err(committed_error) = committed else {
        panic!("a terminal lie fails while the result is stored");
    };
    let committed_log = log.lines();
    let committed_lines = stage_lines(&committed_log, &id);
    assert_eq!(committed_lines.len(), 3, "{committed_error}");
    let fields = &committed_lines[2]["fields"];
    assert_eq!(fields["outcome"], "error", "{committed_error}");
    assert!(fields["execute_ms"].as_u64().is_some_and(|ms| ms >= 10), "{fields}");
    assert!(fields["commit_ms"].as_u64().is_some(), "{fields}");
    assert_eq!(fields["tool_batch_id"], committed_lines[0]["fields"]["tool_batch_id"]);
    assert_eq!(fields["agent_run_id"], committed_lines[0]["fields"]["agent_run_id"]);

    let shared: Vec<_> = log
        .lines()
        .into_iter()
        .filter(|line| {
            line["event"] == "tool.installation_open" && line["fields"]["shared_failure"] == true
        })
        .collect();
    assert_eq!(shared.len(), 1, "{committed_error}");
    let fields = &shared[0]["fields"];
    assert_eq!(fields["ok"], false);
    assert_eq!(fields["published_ms"], 0);
    assert_eq!(fields["package_ms"], 0);
    assert_eq!(fields["images_ms"], 0);
    assert_eq!(fields["cmd_ms"], 0);
    assert_eq!(fields["verify_ms"], 0);
    assert!(fields["wait_ms"].as_u64().is_some());
}

fn absolute_empty(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).expect("Agent test setup and persisted assertion")
}
