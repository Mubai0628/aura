use super::*;

#[tokio::test]
async fn typed_external_process_service_uses_persisted_trust_and_audits_terminals() {
    let directory = tempfile::tempdir().expect("temporary Core service root");
    let database_path = directory.path().join("aura.db");
    let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
        database_path.clone(),
        directory.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("start controlled persistence")
    {
        PersistenceStartup::Ready(coordinator) => coordinator,
        PersistenceStartup::RecoveryRequired(recovery) => {
            panic!("unexpected recovery requirement: {}", recovery.reason)
        }
    };
    let (workspace_id, session_id, permission_snapshot_id) =
        seed_external_process_session(&persistence, directory.path());
    let mut plan =
        managed_networked_plan(workspace_id, session_id, permission_snapshot_id, directory.path());
    persistence
        .upsert_external_trust(
            plan.source.clone(),
            TrustState::Trusted {
                fingerprint: plan.source_fingerprint.clone(),
                trusted_at: aura_domain_common::UtcTimestamp::now(),
            },
            Some(plan.source_fingerprint.clone()),
            CorrelationId::new(),
        )
        .expect("persist trusted MCP source");

    let (client, host, actor) = controlled_sandbox_host(persistence.clone()).await;
    let host_capacity = std::sync::Arc::new(Semaphore::new(1));
    let (service_requests, service_receiver) = mpsc::channel(4);
    tokio::spawn(run_external_process_service(
        ExternalProcessService { sandbox: client.clone(), host_capacity },
        service_receiver,
    ));
    let service = ExternalProcessExecutionService { requests: service_requests };
    let outcome = service
        .execute_external_process(plan.clone())
        .await
        .expect("execute persisted trusted external process");
    assert_eq!(outcome.execution_id, plan.execution_id);
    assert_eq!(outcome.state, SandboxExecutionState::Exited);
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(outcome.network_enforcement, NetworkEnforcementKind::ObservedNativeNotProxyEnforced);
    assert_eq!(external_audit_states(&database_path, plan.execution_id), vec!["planned", "exited"]);

    persistence
        .upsert_external_trust(
            plan.source.clone(),
            TrustState::Revoked,
            Some(plan.source_fingerprint.clone()),
            CorrelationId::new(),
        )
        .expect("persist trust revocation");
    plan.execution_id = SandboxExecutionId::new();
    plan.cancellation_id = CancellationId::new();
    plan.plan_hash = external_process_plan_hash(&plan).expect("rehash revoked plan");
    let rejected = service
        .execute_external_process(plan)
        .await
        .expect_err("Sandbox Host must reload and reject persisted revoked trust");
    let ExternalProcessExecutionError::Rejected(envelope) = rejected else {
        panic!("expected a typed remote rejection")
    };
    assert_eq!(envelope.code.as_str(), "sandbox.external_source_not_trusted");
    assert_eq!(envelope.category, aura_domain_common::ErrorCategory::Permission);
    assert!(!envelope.retryable);
    assert_eq!(envelope.terminal, aura_runtime_contract::IpcRequestTerminal::Failed);
    assert!(!envelope.message.trim().is_empty());
    assert_eq!(
        envelope.details.get("protocol_version"),
        Some(&serde_json::to_value(ContractVersion::CURRENT).expect("protocol version value"))
    );

    let _: Value = client
        .request(method::SIDECAR_SHUTDOWN, &json!({}))
        .await
        .expect("stop controlled Sandbox Host");
    host.await.expect("join Sandbox Host").expect("Sandbox Host terminal");
    actor.abort();
}

fn seed_external_process_session(
    persistence: &PersistenceCoordinator,
    workspace_root: &std::path::Path,
) -> (WorkspaceId, SessionId, PermissionSnapshotId) {
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    let root = workspace_root.display().to_string();
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "Core external service fixture".to_owned(),
                selected_root: root.clone(),
                canonical_root: root,
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: "External process".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("seed workspace and session");
    persistence
        .ensure_session_permission_state(
            session_id,
            workspace_id,
            aura_security_contract::PermissionMode::WorkspaceReadWrite,
            CorrelationId::new(),
        )
        .expect("seed session permission mode");
    let snapshot = persistence
        .create_permission_snapshot(session_id, None, CorrelationId::new())
        .expect("create permission snapshot");
    (workspace_id, session_id, snapshot.id)
}

fn managed_networked_plan(
    workspace_id: WorkspaceId,
    session_id: SessionId,
    permission_snapshot_id: PermissionSnapshotId,
    working_directory: &std::path::Path,
) -> ExternalProcessLaunchPlan {
    let mut identity =
        InvocationIdentity::runtime_user(workspace_id, session_id, CorrelationId::new());
    identity.permission_snapshot_id = Some(permission_snapshot_id);
    let mut plan = ExternalProcessLaunchPlan {
        execution_id: SandboxExecutionId::new(),
        identity,
        permission_snapshot_id,
        source: SecuritySource::McpServer { server_id: "p04-fixture".to_owned() },
        source_fingerprint: ContentHash::blake3(b"p04 controlled MCP fixture"),
        isolation: ExternalIsolationMode::ManagedUserNetworked,
        network_access: NetworkAccessRequirement::Required,
        network_enforcement: NetworkEnforcementKind::ObservedNativeNotProxyEnforced,
        executable: snapshot_executable_ref(
            std::path::Path::new(r"C:\Windows\System32\cmd.exe"),
            ContentHashAlgorithm::Blake3,
        )
        .expect("snapshot cmd.exe"),
        args: ["/d", "/s", "/c", "exit /b 0"]
            .map(|value| WindowsOsString { utf16: value.encode_utf16().collect() })
            .to_vec(),
        working_directory: CanonicalPath {
            display: working_directory.display().to_string(),
            utf16: working_directory.as_os_str().encode_wide().collect(),
        },
        environment: Vec::new(),
        timeout_ms: 5_000,
        output_limits: ProcessOutputLimits {
            stdout_bytes: 1_024,
            stderr_bytes: 1_024,
            combined_bytes: 2_048,
        },
        cancellation_id: CancellationId::new(),
        plan_hash: ContentHash::blake3(b"uninitialized"),
    };
    plan.plan_hash = external_process_plan_hash(&plan).expect("external plan hash");
    plan
}

async fn controlled_sandbox_host(
    persistence: PersistenceCoordinator,
) -> (SidecarClient, JoinHandle<anyhow::Result<()>>, JoinHandle<anyhow::Result<()>>) {
    let pipe_name = format!(r"\\.\pipe\Aura\p04-controlled-{}", RuntimeInstanceId::new());
    let mut server =
        create_secure_byte_pipe_server(&pipe_name, true).expect("create controlled pipe");
    let client_pipe = aura_windows_platform::open_secure_byte_pipe_client(&pipe_name)
        .expect("open controlled pipe with Aura's minimum access mask");
    server.connect().await.expect("connect controlled pipe");
    let host = tokio::spawn(aura_sandbox_host::serve_authenticated_core_commands(
        client_pipe,
        persistence,
    ));
    let (requests, mut request_rx) = mpsc::channel::<SidecarRequest>(16);
    let (events, _) = broadcast::channel(32);
    let actor_events = events.clone();
    let actor = tokio::spawn(async move {
        let mut pending = BTreeMap::<CorrelationId, PendingRequest>::new();
        let mut heartbeat_pending = None;
        loop {
            tokio::select! {
                request = request_rx.recv() => {
                    let request = request.context("controlled request channel closed")?;
                    let id = CorrelationId::new();
                    write_sidecar_request(&mut server, id, request.method_name, request.params)
                        .await?;
                    pending.insert(id, PendingRequest::Caller(request.reply));
                }
                frame = read_frame(&mut server) => {
                    handle_inbound(
                        frame?,
                        &mut pending,
                        &mut heartbeat_pending,
                        &actor_events,
                    )?;
                }
            }
        }
    });
    (SidecarClient { requests, _events: events }, host, actor)
}

fn external_audit_states(
    database_path: &std::path::Path,
    execution_id: SandboxExecutionId,
) -> Vec<String> {
    let connection = Connection::open(database_path).expect("open audit database");
    let mut statement = connection
        .prepare(
            "SELECT payload_json FROM events
                 WHERE event_type='ExternalProcessExecutionAudited'
                 ORDER BY global_sequence",
        )
        .expect("prepare audit query");
    statement
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query external audits")
        .filter_map(|payload| {
            let payload: Value = serde_json::from_str(&payload.expect("external audit payload"))
                .expect("parse external audit payload");
            (payload["sandbox_execution_id"] == execution_id.to_string())
                .then(|| payload["state"].as_str().expect("audit state").to_owned())
        })
        .collect()
}
