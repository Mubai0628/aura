//! Production initialization and request-handler integration, using isolated real SQLite/files.
use crate::*;
use aura_domain_common::SessionId;
use aura_projection_contract::{BackendLifecycle, CapabilityReadiness, CoreHealth};
use aura_workspace_domain::SessionSummary;
use tempfile::TempDir;

mod fixture;
pub(crate) use fixture::Fixture;
mod restart;
// A normal exit, announced by Main, and the Store check after it.
mod clean_exit;

#[tokio::test]
#[ignore = "check-r03 supplies a source-built Sandbox Host and isolated child APPDATA/LOCALAPPDATA"]
async fn r03_real_sidecar_exit_is_local_and_is_not_restarted() {
    let image = PathBuf::from(
        env::var_os("AURA_R03_SANDBOX_IMAGE").expect("source-built backend required"),
    );
    let fixture = Fixture::with_backend(Some(image));
    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else { panic!("Core must start") };
    services
        .security_sidecars_required()
        .tool_sandbox_service
        .ensure_available()
        .await
        .expect("R03 isolated fixture operation must succeed");
    assert!(
        readiness::current(&state)
            .backends
            .iter()
            .any(|b| b.backend == "strict_appcontainer" && b.lifecycle == BackendLifecycle::Ready)
    );
    services
        .security_sidecars_required()
        .shutdown_sandbox_for_test()
        .await
        .expect("R03 isolated fixture operation must succeed");
    time::timeout(Duration::from_secs(10), async {
        loop {
            if readiness::current(&state).backends.iter().any(|b| {
                b.backend == "strict_appcontainer" && b.lifecycle == BackendLifecycle::Degraded
            }) {
                break;
            }
            time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("R03 isolated fixture operation must succeed");
    assert!(
        services
            .security_sidecars_required()
            .tool_sandbox_service
            .ensure_available()
            .await
            .is_err()
    );
    assert_eq!(success(&state, "runtime.readiness", json!({})).await["core_health"], "ready");
    assert!(success(&state, "models.settings.get_snapshot", json!({})).await.is_object());
    let directory = fixture.root.path().join("after-exit");
    fs::create_dir(&directory).expect("R03 isolated fixture operation must succeed");
    fs::write(directory.join("read.txt"), "after backend exit")
        .expect("R03 isolated fixture operation must succeed");
    let created = success(
        &state,
        method::WORKSPACE_CREATE,
        json!({"selected_root":directory,"display_name":"After exit"}),
    )
    .await;
    let workspace: WorkspaceSummary = serde_json::from_value(created["workspace"].clone())
        .expect("R03 isolated fixture operation must succeed");
    let session_id = SessionId::new();
    services
        .persistence
        .upsert_workspace_session(
            workspace.clone(),
            SessionSummary {
                session_id,
                workspace_id: workspace.workspace_id,
                title: "history after exit".into(),
                state: "idle".into(),
            },
            CorrelationId::new(),
        )
        .expect("R03 isolated fixture operation must succeed");
    let file=success(&state,method::FS_READ_TEXT,json!({"workspace_id":workspace.workspace_id,"session_id":session_id,"target":{"path":"read.txt"},"range":{"range_kind":"full"}})).await;
    assert_eq!(file["text"], "after backend exit");
    assert!(
        success(
            &state,
            method::PROJECTION_SNAPSHOT,
            json!({"projection_kind":"workspace_session","projection_scope":{"type":"global"}})
        )
        .await
        .to_string()
        .contains("history after exit")
    );
}

pub(crate) async fn request(state: &RuntimeState, method_name: &str, params: Value) -> Value {
    let queue = AsyncPriorityQueue::new();
    let request = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: is_side_effecting_main_method(method_name)
            .then(CommandInvocationId::new),
        method: method_name.into(),
        params,
    };
    // This helper reaches the handler directly, so it must take the claim the
    // same way the read loop does: a request without the credential that
    // authorises it is refused, by design.
    let claim = match crate::reserve_command_invocation(state, &queue, &request)
        .await
        .expect("the fixture's claim entry")
    {
        crate::CommandReservation::Granted(claim) => claim,
        crate::CommandReservation::Refused => {
            panic!("{method_name}: the isolated fixture refused to claim its own invocation id")
        }
    };
    handle_request(
        state,
        &queue,
        &ProjectionSubscriptions::default(),
        &Arc::new(std::sync::Mutex::new(RequestLedger::default())),
        &request,
        claim.as_ref(),
    )
    .await
    .expect("R03 isolated fixture operation must succeed");
    loop {
        let WireFrame::Control(bytes) =
            queue.recv().await.expect("R03 isolated fixture operation must succeed").frame
        else {
            panic!()
        };
        let response: Value =
            serde_json::from_slice(&bytes).expect("R03 isolated fixture operation must succeed");
        // User terminal opening progress is a real notification before its reply.
        if response["id"]
            == serde_json::to_value(request.id).expect("R03 isolated request ID must serialize")
        {
            return response;
        }
    }
}

pub(crate) async fn success(state: &RuntimeState, method_name: &str, params: Value) -> Value {
    let response = request(state, method_name, params).await;
    assert!(response.get("error").is_none(), "{method_name}: {response}");
    response["result"]["value"].clone()
}

/// The outcome of a request that may be refused **before** dispatch, as well as by it.
///
/// [`request`] panics when the handler returns a transport-level error, which is the honest
/// shape for a request whose parameters do not deserialize at all. A caller that needs to
/// distinguish "refused as a malformed request" from "refused as a valid request with a real
/// reason" observes both here rather than being unable to see the first.
pub(crate) async fn attempt(
    state: &RuntimeState,
    method_name: &str,
    params: Value,
) -> Result<Value, String> {
    let queue = AsyncPriorityQueue::new();
    let request = JsonRpcRequest {
        jsonrpc: JSON_RPC_VERSION.into(),
        id: CorrelationId::new(),
        command_invocation_id: is_side_effecting_main_method(method_name)
            .then(CommandInvocationId::new),
        method: method_name.into(),
        params,
    };
    let claim = match crate::reserve_command_invocation(state, &queue, &request)
        .await
        .map_err(|error| error.to_string())?
    {
        crate::CommandReservation::Granted(claim) => claim,
        crate::CommandReservation::Refused => {
            return Err(format!("{method_name}: the fixture refused to claim its invocation id"));
        }
    };
    if let Err(error) = handle_request(
        state,
        &queue,
        &ProjectionSubscriptions::default(),
        &Arc::new(std::sync::Mutex::new(RequestLedger::default())),
        &request,
        claim.as_ref(),
    )
    .await
    {
        return Err(error.to_string());
    }
    let WireFrame::Control(bytes) = queue
        .recv()
        .await
        .map_err(|_| format!("{method_name}: the handler wrote no response"))?
        .frame
    else {
        return Err(format!("{method_name}: the handler wrote a non-control frame"));
    };
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

/// Isolated Core fixtures still register stable Backend IDs. Restricted
/// readiness follows the host installation, Native may be Ready, and neither
/// is required for Core Broker file/history routes.
pub(crate) fn assert_core_broker_ready_without_requiring_process_backends(state: &RuntimeState) {
    let snapshot = readiness::current(state);
    let broker = snapshot
        .backends
        .iter()
        .find(|backend| backend.backend == "core_broker")
        .expect("core_broker status");
    assert_eq!(broker.lifecycle, BackendLifecycle::Ready);
    assert!(
        snapshot.capabilities.iter().any(|capability| {
            capability.capability == "workspace.files"
                && capability.readiness == CapabilityReadiness::Ready
        }),
        "workspace.files must stay ready without a selected process backend"
    );
}

#[tokio::test]
async fn r03_missing_backend_preserves_real_configuration_files_and_persisted_history() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else { panic!("Core must be ready") };
    assert_core_broker_ready_without_requiring_process_backends(&state);
    let workspace_root = fixture.root.path().join("workspace");
    fs::create_dir(&workspace_root).expect("R03 isolated fixture operation must succeed");
    fs::write(workspace_root.join("hello.txt"), "persisted file contents")
        .expect("R03 isolated fixture operation must succeed");
    let created = success(
        &state,
        method::WORKSPACE_CREATE,
        json!({"selected_root":workspace_root,"display_name":"R03"}),
    )
    .await;
    let workspace: WorkspaceSummary = serde_json::from_value(created["workspace"].clone())
        .expect("R03 isolated fixture operation must succeed");
    let session_id = SessionId::new();
    services
        .persistence
        .upsert_workspace_session(
            workspace.clone(),
            SessionSummary {
                session_id,
                workspace_id: workspace.workspace_id,
                title: "persisted history".into(),
                state: "idle".into(),
            },
            CorrelationId::new(),
        )
        .expect("R03 isolated fixture operation must succeed");
    assert!(
        services
            .security_sidecars_required()
            .tool_sandbox_service
            .ensure_available()
            .await
            .is_err()
    );
    // A second request cannot restart or transparently replay the failed backend.
    assert!(
        services
            .security_sidecars_required()
            .tool_sandbox_service
            .ensure_available()
            .await
            .is_err()
    );
    let readiness = success(&state, "runtime.readiness", json!({})).await;
    assert_eq!(readiness["core_health"], "ready");
    assert!(
        readiness["backends"]
            .as_array()
            .expect("R03 isolated fixture operation must succeed")
            .iter()
            .any(|b| b["backend"] == "strict_appcontainer" && b["lifecycle"] == "degraded")
    );
    let settings = success(&state, "models.settings.get_snapshot", json!({})).await;
    assert!(settings.is_object());
    let file=success(&state,method::FS_READ_TEXT,json!({"workspace_id":workspace.workspace_id,"session_id":session_id,"target":{"path":"hello.txt"},"range":{"range_kind":"full"},"explicit_encoding":null})).await;
    assert_eq!(file["text"], "persisted file contents");
    // The authenticated production mutation route uses backend-neutral
    // policy, while the actual filesystem service enforces paths and journals.
    success(&state,method::FS_CREATE_DIRECTORY,json!({"workspace_id":workspace.workspace_id,"session_id":session_id,"target":{"path":"policy-v2"}})).await;
    assert!(workspace_root.join("policy-v2").is_dir());
    let escaped_write=request(&state,method::FS_CREATE_DIRECTORY,json!({"workspace_id":workspace.workspace_id,"session_id":session_id,"target":{"path":"../policy-escape"}})).await;
    assert!(escaped_write.get("error").is_some());
    assert!(!fixture.root.path().join("policy-escape").exists());
    let history = success(
        &state,
        method::PROJECTION_SNAPSHOT,
        json!({"projection_kind":"workspace_session","projection_scope":{"type":"global"}}),
    )
    .await;
    assert!(history.to_string().contains("persisted history"));
    let escaped=request(&state,method::FS_READ_TEXT,json!({"workspace_id":workspace.workspace_id,"session_id":session_id,"target":{"path":"../escape.txt"},"range":{"range_kind":"full"}})).await;
    assert!(escaped.get("error").is_some());
    drop(state);
    // Same actual database is reopened; startup recovery does not need a backend.
    let restarted = fixture.start().await;
    assert_eq!(readiness::current(&restarted).core_health, CoreHealth::Ready);
    assert!(
        success(
            &restarted,
            method::PROJECTION_SNAPSHOT,
            json!({"projection_kind":"workspace_session","projection_scope":{"type":"global"}})
        )
        .await
        .to_string()
        .contains("persisted history")
    );
}

#[tokio::test]
async fn r03_real_database_corruption_still_requires_global_recovery() {
    let fixture = Fixture::new();
    fixture.paths.ensure_layout().expect("R03 isolated fixture operation must succeed");
    fs::write(&fixture.paths.database_path, b"not a SQLite database")
        .expect("R03 isolated fixture operation must succeed");
    let state = fixture.start().await;
    assert!(matches!(state.startup_mode, StartupMode::RecoveryRequired { .. }));
    assert_eq!(
        success(&state, "runtime.readiness", json!({})).await["core_health"],
        "recovery_required"
    );
    assert!(
        request(&state, "models.settings.get_snapshot", json!({})).await.get("error").is_some()
    );
}

/// The search flight registry is process-global, so every test that registers a
/// search — or that counts the registry — must hold this. Without it, one test's
/// live registration is another test's evidence.
pub(crate) fn search_flight_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

#[tokio::test]
async fn connection_cleanup_releases_only_this_connections_registrations() {
    // The search flight registry and the command ledger are process-global, so
    // cleanup must go through the ids *this* connection owns. Reaching further
    // would cancel or release work another connection is still running.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    // Taken after the last await: a std guard must not be held across one.
    let _serial = search_flight_lock();
    let ledger = Arc::new(std::sync::Mutex::new(RequestLedger::new()));
    let mine = CorrelationId::new();
    let theirs = CorrelationId::new();
    aura_fs_tools::register_search(mine, aura_fs_tools::SearchCancel::new());
    aura_fs_tools::register_search(theirs, aura_fs_tools::SearchCancel::new());
    ledger.lock().expect("ledger").begin(mine).expect("this connection's request is running");
    let mut search_ids = BTreeSet::from([mine]);
    let mut command_flights = BTreeMap::new();
    let invocation = CommandInvocationId::new();
    let claim = state
        .command_outcomes
        .lock()
        .expect("outcomes")
        .begin(invocation, mine)
        .expect("the command claim starts out owned by this connection");
    command_flights.insert(mine, claim);

    let queue = aura_ipc::AsyncPriorityQueue::new();
    // A search registration is released on *real* completion — the coordinator
    // returned and no admitted work is left — not merely because its handler
    // finished. Model the completed search accordingly.
    if let Some(cancel) = aura_fs_tools::search_flight(mine) {
        cancel.mark_coordinator_done();
    }
    let panicked = finish_handled_requests(
        &state,
        &queue,
        &ledger,
        &mut search_ids,
        &mut command_flights,
        vec![(mine, ipc_session::HandlerTerminal::Answered(IpcRequestTerminal::Succeeded))],
    )
    .expect("finishing one handler must succeed");

    assert!(panicked.is_empty(), "a normal terminal is not a panic");
    assert!(search_ids.is_empty(), "this connection's search id is released");
    assert!(command_flights.is_empty(), "this connection's command claim is released");
    assert!(
        aura_fs_tools::search_flight(mine).is_none(),
        "this connection's cancelled search is no longer registered"
    );
    assert!(
        aura_fs_tools::search_flight(theirs).is_some(),
        "another connection's search must stay registered and cancellable"
    );
    assert!(
        !state.command_outcomes.lock().expect("outcomes").is_in_flight(invocation),
        "the released claim does not stay in flight"
    );
    aura_fs_tools::cancel_search(theirs);
    // Let the other connection's search finish before releasing it, so this test
    // leaves no registration behind for tests that share the registry.
    if let Some(cancel) = aura_fs_tools::search_flight(theirs) {
        cancel.mark_coordinator_done();
    }
    assert!(aura_fs_tools::release_search_flight(theirs));
}

#[test]
fn r03_readiness_backend_changes_do_not_override_core_integrity() {
    let store = readiness::ReadinessStore::default();
    store.update("strict_appcontainer", BackendLifecycle::Ready, "backend.ready");
    store.update("egress_proxy", BackendLifecycle::Degraded, "backend.transport_lost");
    let ready = store.snapshot(CoreHealth::Ready);
    assert!(
        ready
            .capabilities
            .iter()
            .filter(|c| c.capability != "process.execution")
            .all(|c| c.readiness == aura_projection_contract::CapabilityReadiness::Ready)
    );
    let recovery = store.snapshot(CoreHealth::RecoveryRequired);
    assert!(
        recovery
            .capabilities
            .iter()
            .all(|c| c.readiness == aura_projection_contract::CapabilityReadiness::Unavailable)
    );
}
