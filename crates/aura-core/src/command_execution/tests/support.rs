use super::*;
use aura_agent_execution::security_control;
use aura_core_base::sync_boundary;
use aura_core_broker::agent_store;

pub(super) struct Case {
    pub(super) _root: tempfile::TempDir,
    pub(super) port: CorePort,
    pub(super) request: DispatchRequest,
}

impl Case {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let work = root.path().join("workspace");
        std::fs::create_dir(&work).unwrap();
        let fixture = aura_contract_schema::v2_kernel_fixture().unwrap();
        let mut content = fixture.dispatch.operation.content;
        content.credentials.clear();
        content.identity.actor = InvocationActor::ParentAgent;
        content.identity.agent_run_id = Some(AgentRunId::new());
        content.identity.turn_task_id = Some(TurnTaskId::new());
        content.identity.permission_snapshot_id = Some(PermissionSnapshotId::new());
        content.source = SecuritySourceRef::BuiltInTool {
            identity_hash: jcs::CanonicalSha256HashV1::sha256(b"managed-shell-test"),
        };
        content.capabilities =
            vec![ResolvedCapability::ProcessExecute, ResolvedCapability::WorkspaceFileRead];
        content.action_kind = "shell.powershell".into();
        content.action = serde_json::to_value(ManagedShellCommand {
            shell: aura_windows_user_contract::ManagedShell::PowerShell,
            script: "'value'".into(),
            working_directory: work.to_string_lossy().into_owned(),
            stdin: String::new(),
            timeout_ms: 1000,
            max_output_bytes: 1024,
            secret_environment: Vec::new(),
        })
        .unwrap();
        let operation = SecuredOperation::new(SecurityOperationId::new(), content).unwrap();
        let now = UtcTimestamp::now();
        let expires = UtcTimestamp::from_value(now.value() + time::Duration::minutes(1));
        let workspace_id = operation.content.identity.workspace_id.unwrap();
        let session_id = operation.content.identity.session_id.unwrap();
        let (binding, _) = aura_backend_windows_user::file_binding::capture(&work, false).unwrap();
        let workspace = aura_workspace_domain::WorkspaceSummary {
            workspace_id,
            display_name: "command fixture".into(),
            selected_root: binding.path.clone(),
            canonical_root: binding.path.clone(),
            root_identity: Some(FileIdentity {
                volume_serial_number: binding.volume_serial.to_string(),
                file_id: binding.file_id.iter().map(|v| format!("{v:02x}")).collect(),
            }),
            state: aura_workspace_domain::WorkspaceState::Online,
            revision: Revision::INITIAL,
            catalog_freshness: aura_workspace_domain::CatalogFreshness::Unavailable,
            authorization_epoch: Revision::INITIAL,
        };
        let aura_persistence::PersistenceStartup::Ready(persistence) =
            aura_persistence::PersistenceCoordinator::start(
                aura_persistence::PersistenceOptions::new(
                    root.path().join("current/aura.db"),
                    root.path().join("current/artifacts"),
                    RuntimeInstanceId::new(),
                ),
            )
            .unwrap()
        else {
            panic!("recovery")
        };
        persistence
            .upsert_workspace_session(
                workspace.clone(),
                aura_workspace_domain::SessionSummary {
                    session_id,
                    workspace_id,
                    title: "command fixture".into(),
                    state: "idle".into(),
                },
                CorrelationId::new(),
            )
            .unwrap();
        persistence
            .ensure_session_permission_state(
                session_id,
                workspace_id,
                PermissionMode::WorkspaceReadWrite,
                CorrelationId::new(),
            )
            .unwrap();
        let journal =
            Arc::new(crate::broker::BrokerJournal::open(&root.path().join("v2")).unwrap());
        let snapshot = PermissionSnapshot {
            id: operation.content.identity.permission_snapshot_id.unwrap(),
            workspace_id,
            session_id,
            mode: PermissionMode::WorkspaceReadWrite,
            session_permission_revision: 1,
            active_grant_ids: vec![],
            created_at: now,
        };
        journal
            .lock()
            .unwrap()
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::workspace::insert(tx, &workspace)?;
                aura_persistence_v2::workspace::insert_session(
                    tx,
                    &aura_persistence_v2::workspace::SessionMetadata {
                        id: session_id,
                        workspace_id,
                        title: "command fixture".into(),
                        created_at: now,
                        updated_at: now,
                    },
                )?;
                aura_persistence_v2::security::insert_snapshot(tx, &snapshot)
            })
            .unwrap();
        let mut backend = fixture.dispatch.plan.prepared.backend;
        backend.class = BackendClass::RestrictedUserOffline;
        backend.id = BackendId("command-unit-fixture".into());
        backend.capabilities = operation.content.capabilities.iter().copied().collect();
        backend.known_limitations = [BackendKnownLimitation::DnscacheServiceMediatedDns].into();
        backend.guarantees = [
            EnforcementGuarantee::AuthorizedWritesOnly,
            EnforcementGuarantee::IdentityIsolation,
            EnforcementGuarantee::DirectNetworkBlockedV1,
            EnforcementGuarantee::ChildProcessControl,
            EnforcementGuarantee::BoundedOutput,
        ]
        .into();
        let input = aura_permission_engine::policy::PolicyInput {
            network_caller: NetworkCallerKind::AuraTool,
            operation: &operation,
            snapshot: &snapshot,
            profile: SecurityProfile::Balanced,
            source_enabled: true,
            trust: &TrustState::SystemTrusted,
            source_fingerprint: None,
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &[],
            verified_external_grants: &[],
            verified_external_capabilities: &[],
            now,
            expires_at: expires,
            core_runtime_instance_id: Some(RuntimeInstanceId::new()),
        };
        let PolicyAuthorization::Allow { authorization } =
            aura_permission_engine::policy::authorize(&input, &backend, None)
        else {
            panic!("mode should allow")
        };
        let preparation = WindowsUserPreparation {
            schema_version: ToolWireVersionV1,
            assurance_version: ASSURANCE_VERSION.into(),
            shell_environment_version: SHELL_ENVIRONMENT_VERSION.into(),
            installation_fingerprint: jcs::CanonicalSha256HashV1::sha256(b"no-host-fixture"),
            mode: snapshot.mode,
            permission_revision: 1,
            valid_until: expires,
            account_sid: "unit-fixture".into(),
            shell_image: binding.clone(),
            working_directory: binding.clone(),
            read_roots: vec![binding.clone()],
            write_roots: vec![binding],
        };
        let plan = ExecutionPlan::new(PreparedExecution {
            operation_fingerprint: operation.fingerprint.clone(),
            backend: backend.clone(),
            preparation: serde_json::to_value(preparation).unwrap(),
        })
        .unwrap();
        let request = DispatchRequest {
            operation: operation.clone(),
            binding: DispatchBinding {
                attempt: AttemptRef {
                    operation_id: operation.operation_id,
                    ordinal: std::num::NonZeroU32::MIN,
                },
                operation_fingerprint: operation.fingerprint.clone(),
                authorization_fingerprint: authorization.fingerprint().unwrap(),
                plan_fingerprint: plan.fingerprint.clone(),
                cancellation_revision: Revision::INITIAL,
            },
            plan,
        };
        security_control::reserve_dispatch(
            &persistence,
            &mut journal.lock().unwrap(),
            request.clone(),
            &authorization,
            &input,
            &backend,
            &CancellationState { revision: Revision::INITIAL, requested: false },
            "command-fixture",
        )
        .unwrap();
        Self {
            _root: root,
            port: CorePort {
                persistence: Arc::new(persistence),
                // The port's bounded owner. A test rig shares the one boundary the
                // rest of Core's test fixture uses; the sections below still run whole on it.
                store: Arc::new(agent_store::AgentStore::new(
                    Arc::clone(&journal),
                    sync_boundary::SyncBoundary::open(),
                )),
                journal,
                // The fixture's own index, so a test can observe the real publication
                // without sharing one with another test running in the same process.
                activity_index: aura_activity_runtime::ActivityIndex::default(),
                activity_observer: aura_activity_runtime::ActivityObserverHandle::default(),
                background_observations: Arc::new(
                    super::background_observations::BackgroundObservations::default(),
                ),
            },
            request,
        }
    }
}
