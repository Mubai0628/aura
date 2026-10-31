//! Actual Core policy + V2 SQLite transaction integration; no runner simulation.
use aura_agent_execution::security_control::reserve_dispatch;
use aura_domain_common::{ApprovalRequestId, PermissionSnapshotId, Revision};
use aura_execution_contract::*;
use aura_permission_engine::policy::{PolicyInput, authorize};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_persistence_v2::{V2Store, security, security_approval, workspace};
use aura_security_contract::*;
use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};

struct Case {
    root: tempfile::TempDir,
    authority: aura_persistence::PersistenceCoordinator,
    store: V2Store,
    op: SecuredOperation,
    snapshot: PermissionSnapshot,
    backend: BackendDescriptor,
}
impl Case {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary namespace");
        let mut store = V2Store::create(&root.path().join("v2")).expect("isolated V2 store");
        let f = aura_contract_schema::v2_kernel_fixture().expect("fixture");
        let mut op = f.dispatch.operation;
        op.content.credentials.clear();
        op.content.identity.actor = InvocationActor::ParentAgent;
        op.content.identity.agent_run_id = Some(aura_domain_common::AgentRunId::new());
        op.content.identity.turn_task_id = Some(aura_domain_common::TurnTaskId::new());
        op.content.source =
            SecuritySourceRef::BuiltInTool { identity_hash: op.fingerprint.0.clone() };
        let snapshot = PermissionSnapshot {
            id: PermissionSnapshotId::new(),
            workspace_id: op.content.identity.workspace_id.expect("workspace"),
            session_id: op.content.identity.session_id.expect("session"),
            mode: PermissionMode::WorkspaceReadWrite,
            session_permission_revision: 1,
            active_grant_ids: vec![],
            created_at: f.authority.decided_at,
        };
        op.content.identity.permission_snapshot_id = Some(snapshot.id);
        op = SecuredOperation::new(op.operation_id, op.content).expect("operation");
        store
            .database
            .unit_of_work(|tx| {
                workspace::insert(
                    tx,
                    &WorkspaceSummary {
                        workspace_id: snapshot.workspace_id,
                        display_name: "R04".into(),
                        selected_root: root.path().display().to_string(),
                        canonical_root: root.path().display().to_string(),
                        root_identity: None,
                        state: WorkspaceState::Online,
                        revision: Revision::INITIAL,
                        catalog_freshness: CatalogFreshness::Unavailable,
                        authorization_epoch: Revision::INITIAL,
                    },
                )?;
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id: snapshot.session_id,
                        workspace_id: snapshot.workspace_id,
                        title: "Security".into(),
                        created_at: snapshot.created_at,
                        updated_at: snapshot.created_at,
                    },
                )?;
                security::insert_snapshot(tx, &snapshot)
            })
            .expect("domain stores");
        let options = aura_persistence::PersistenceOptions::new(
            root.path().join("current/aura.db"),
            root.path().join("current/artifacts"),
            aura_domain_common::RuntimeInstanceId::new(),
        );
        let aura_persistence::PersistenceStartup::Ready(authority) =
            aura_persistence::PersistenceCoordinator::start(options).expect("mode authority")
        else {
            panic!("unexpected recovery")
        };
        authority
            .upsert_workspace_session(
                WorkspaceSummary {
                    workspace_id: snapshot.workspace_id,
                    display_name: "Mode authority".into(),
                    selected_root: root.path().display().to_string(),
                    canonical_root: root.path().display().to_string(),
                    root_identity: None,
                    state: WorkspaceState::Online,
                    revision: Revision::INITIAL,
                    catalog_freshness: CatalogFreshness::Unavailable,
                    authorization_epoch: Revision::INITIAL,
                },
                aura_workspace_domain::SessionSummary {
                    session_id: snapshot.session_id,
                    workspace_id: snapshot.workspace_id,
                    title: "Security".into(),
                    state: "idle".into(),
                },
                aura_domain_common::CorrelationId::new(),
            )
            .expect("UI session");
        authority
            .ensure_session_permission_state(
                snapshot.session_id,
                snapshot.workspace_id,
                snapshot.mode,
                aura_domain_common::CorrelationId::new(),
            )
            .expect("UI mode");
        Self { root, authority, store, op, snapshot, backend: f.dispatch.plan.prepared.backend }
    }
    fn input<'a>(op: &'a SecuredOperation, snapshot: &'a PermissionSnapshot) -> PolicyInput<'a> {
        PolicyInput {
            network_caller: aura_security_contract::NetworkCallerKind::AuraTool,
            operation: op,
            snapshot,
            profile: SecurityProfile::Balanced,
            source_enabled: true,
            trust: &TrustState::SystemTrusted,
            source_fingerprint: None,
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &[],
            verified_external_grants: &[],
            verified_external_capabilities: &[],
            now: snapshot.created_at,
            expires_at: "2026-09-07T01:00:00Z".parse().expect("expiry"),
            // These cases carry no reusable consent, so naming this process is
            // enough to make the consent branch fail closed.
            core_runtime_instance_id: Some(aura_domain_common::RuntimeInstanceId::new()),
        }
    }
    fn request(&self, authority: &AuthorizationDecision) -> DispatchRequest {
        let plan = ExecutionPlan::new(PreparedExecution {
            operation_fingerprint: self.op.fingerprint.clone(),
            backend: self.backend.clone(),
            preparation: serde_json::json!({"file_identity":"actual-core-resolver-test"}),
        })
        .expect("plan");
        DispatchRequest {
            operation: self.op.clone(),
            binding: DispatchBinding {
                attempt: AttemptRef {
                    operation_id: self.op.operation_id,
                    ordinal: std::num::NonZeroU32::MIN,
                },
                operation_fingerprint: self.op.fingerprint.clone(),
                authorization_fingerprint: authority.fingerprint().expect("authority"),
                plan_fingerprint: plan.fingerprint.clone(),
                cancellation_revision: Revision::INITIAL,
            },
            plan,
        }
    }
}
fn cancellation() -> CancellationState {
    CancellationState { revision: Revision::INITIAL, requested: false }
}

#[test]
fn ui_mode_change_invalidates_v2_dispatch_without_a_new_snapshot() {
    let mut c = Case::new();
    // A mode change withdraws a prepared dispatch only when
    // the new default no longer covers what the operation needs. A workspace write loses its
    // default in ReadOnly, so the stale dispatch here is a write; a read that ReadOnly still
    // covers keeps its dispatch (see `ui_mode_tightening_keeps_a_dispatch_the_new_default_covers`).
    c.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileWrite];
    c.op = SecuredOperation::new(c.op.operation_id, c.op.content.clone()).expect("workspace write");
    c.backend.capabilities.insert(ResolvedCapability::WorkspaceFileWrite);
    let PolicyAuthorization::Allow { authorization } =
        authorize(&Case::input(&c.op, &c.snapshot), &c.backend, None)
    else {
        panic!("allow")
    };
    let request = c.request(&authorization);
    c.authority
        .set_session_permission_mode(
            c.snapshot.session_id,
            1,
            PermissionMode::ReadOnly,
            aura_domain_common::CorrelationId::new(),
        )
        .expect("UI setter");
    assert!(
        security::validate_snapshot_binding(c.store.database.connection(), &c.snapshot).is_ok(),
        "historical journal is unchanged"
    );
    assert!(
        reserve_dispatch(
            &c.authority,
            &mut c.store,
            request,
            &authorization,
            &Case::input(&c.op, &c.snapshot),
            &c.backend,
            &cancellation(),
            "stale-mode"
        )
        .is_err()
    );
    let count: i64 = c
        .store
        .database
        .connection()
        .query_row("SELECT count(*) FROM execution_attempts", [], |r| r.get(0))
        .expect("attempts");
    assert_eq!(count, 0);
}

/// The counterpart of the stale write above: tightening must not withdraw a dispatch whose
/// capability the new default still covers, or a mode change would cancel unaffected work.
#[test]
fn ui_mode_tightening_keeps_a_dispatch_the_new_default_covers() {
    let mut c = Case::new();
    assert_eq!(c.op.content.capabilities, vec![ResolvedCapability::WorkspaceFileRead]);
    let PolicyAuthorization::Allow { authorization } =
        authorize(&Case::input(&c.op, &c.snapshot), &c.backend, None)
    else {
        panic!("allow")
    };
    let request = c.request(&authorization);
    c.authority
        .set_session_permission_mode(
            c.snapshot.session_id,
            1,
            PermissionMode::ReadOnly,
            aura_domain_common::CorrelationId::new(),
        )
        .expect("UI setter");
    reserve_dispatch(
        &c.authority,
        &mut c.store,
        request,
        &authorization,
        &Case::input(&c.op, &c.snapshot),
        &c.backend,
        &cancellation(),
        "covered-read",
    )
    .expect("a read that ReadOnly still covers keeps its dispatch");
}

#[test]
fn r04_core_reservation_reloads_snapshot_and_rejects_replay_after_restart() {
    let mut c = Case::new();
    let PolicyAuthorization::Allow { authorization } =
        authorize(&Case::input(&c.op, &c.snapshot), &c.backend, None)
    else {
        panic!("baseline allow")
    };
    let request = c.request(&authorization);
    reserve_dispatch(
        &c.authority,
        &mut c.store,
        request.clone(),
        &authorization,
        &Case::input(&c.op, &c.snapshot),
        &c.backend,
        &cancellation(),
        "request-1",
    )
    .expect("atomic prepared dispatch");
    let reopened = V2Store::open(&c.root.path().join("v2")).expect("reopen");
    c.store = reopened;
    let mut replay = request.clone();
    replay.binding.attempt.ordinal = std::num::NonZeroU32::new(2).expect("ordinal");
    assert!(
        reserve_dispatch(
            &c.authority,
            &mut c.store,
            replay,
            &authorization,
            &Case::input(&c.op, &c.snapshot),
            &c.backend,
            &cancellation(),
            "request-2"
        )
        .is_err()
    );
    let count: i64 = c
        .store
        .database
        .connection()
        .query_row("SELECT count(*) FROM execution_attempts", [], |r| r.get(0))
        .expect("attempt count");
    assert_eq!(count, 1);
    let mut newer = c.snapshot.clone();
    newer.id = PermissionSnapshotId::new();
    newer.session_permission_revision += 1;
    newer.mode = PermissionMode::ReadOnly;
    c.store
        .database
        .unit_of_work(|tx| security::insert_snapshot(tx, &newer))
        .expect("new current permission");
    assert!(
        security::validate_snapshot_binding(c.store.database.connection(), &c.snapshot).is_err()
    );
    assert!(
        reserve_dispatch(
            &c.authority,
            &mut c.store,
            request,
            &authorization,
            &Case::input(&c.op, &c.snapshot),
            &c.backend,
            &cancellation(),
            "request-1"
        )
        .is_err()
    );
}

#[test]
fn r04_approval_choice_expiry_replay_and_transaction_rollback() {
    let mut c = Case::new();
    let PolicyAuthorization::Allow { authorization } =
        authorize(&Case::input(&c.op, &c.snapshot), &c.backend, None)
    else {
        panic!("allow")
    };
    let approval = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: authorization.scope,
        allowed_decisions: vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
        state: ApprovalState::Pending,
        requested_at: c.snapshot.created_at,
        expires_at: "2026-09-07T01:00:00Z".parse().expect("expiry"),
    };
    c.store
        .database
        .unit_of_work(|tx| security::insert_approval(tx, &approval))
        .expect("pending approval");
    let resolve = |tx: &rusqlite::Transaction<'_>, choice, now| {
        security_approval::resolve(
            tx,
            approval.approval_id,
            Revision::INITIAL,
            &c.op,
            choice,
            // The fixture backend is the Core Broker class, so a session approval here
            // keeps its exact-operation meaning and never mints a Native consent.
            aura_security_contract::BackendClass::CoreBroker,
            c.snapshot.session_permission_revision,
            aura_domain_common::RuntimeInstanceId::new(),
            now,
        )
    };
    assert!(
        c.store
            .database
            .unit_of_work(|tx| resolve(
                tx,
                ApprovalDecisionKind::AllowWorkspace,
                approval.requested_at
            ))
            .is_err()
    );
    assert!(
        c.store
            .database
            .unit_of_work(|tx| resolve(tx, ApprovalDecisionKind::AllowOnce, approval.expires_at))
            .is_err()
    );
    let failed: StoreResult<()> = c.store.database.unit_of_work(|tx| {
        resolve(tx, ApprovalDecisionKind::AllowOnce, approval.requested_at)?;
        Err(Error::Invalid("interrupt before commit"))
    });
    assert!(failed.is_err());
    assert_eq!(
        security_approval::load(c.store.database.connection(), approval.approval_id)
            .expect("pending after rollback")
            .state,
        ApprovalState::Pending
    );
    let grant = c
        .store
        .database
        .unit_of_work(|tx| resolve(tx, ApprovalDecisionKind::AllowOnce, approval.requested_at))
        .expect("resolve")
        .expect("grant");
    assert!(
        c.store
            .database
            .unit_of_work(|tx| resolve(tx, ApprovalDecisionKind::AllowOnce, approval.requested_at))
            .is_err()
    );
    // Reloaded grant state, rather than the caller's cached list, controls dispatch.
    let authority =
        AuthorizationDecision::from_grant(&grant, approval.requested_at, approval.expires_at)
            .expect("grant decision");
    let request = c.request(&authority);
    c.store
        .database
        .unit_of_work(|tx| {
            security::revoke(tx, grant.grant_id, grant.revision, None, approval.requested_at)
        })
        .expect("revoke");
    assert!(
        reserve_dispatch(
            &c.authority,
            &mut c.store,
            request,
            &authority,
            &Case::input(&c.op, &c.snapshot),
            &c.backend,
            &cancellation(),
            "revoked-request"
        )
        .is_err()
    );
    let count: i64 = c
        .store
        .database
        .connection()
        .query_row("SELECT count(*) FROM execution_attempts", [], |r| r.get(0))
        .expect("attempts");
    assert_eq!(count, 0);
}

#[test]
fn r04_new_once_grant_is_loaded_without_rewriting_operation_or_snapshot() {
    let mut c = Case::new();
    // Removed the extra question for an ordinary delete in a writable scope, so the
    // high-risk trigger here is a system-management change, which still asks on its own.
    c.op.content.risks.push(OperationRiskFlag::SystemConfigurationMutation);
    c.op = SecuredOperation::new(c.op.operation_id, c.op.content.clone())
        .expect("exact risky operation");
    let PolicyAuthorization::Ask { proposal } =
        authorize(&Case::input(&c.op, &c.snapshot), &c.backend, None)
    else {
        panic!("high risk asks")
    };
    let approval = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: c.snapshot.created_at,
        expires_at: Case::input(&c.op, &c.snapshot).expires_at,
    };
    let grant = c
        .store
        .database
        .unit_of_work(|tx| {
            security::insert_approval(tx, &approval)?;
            security_approval::resolve(
                tx,
                approval.approval_id,
                Revision::INITIAL,
                &c.op,
                ApprovalDecisionKind::AllowOnce,
                aura_security_contract::BackendClass::CoreBroker,
                c.snapshot.session_permission_revision,
                aura_domain_common::RuntimeInstanceId::new(),
                approval.requested_at,
            )
        })
        .expect("approve")
        .expect("once grant");
    assert!(c.snapshot.active_grant_ids.is_empty());
    let authority =
        AuthorizationDecision::from_grant(&grant, approval.requested_at, approval.expires_at)
            .expect("authority");
    let request = c.request(&authority);
    reserve_dispatch(
        &c.authority,
        &mut c.store,
        request.clone(),
        &authority,
        &Case::input(&c.op, &c.snapshot),
        &c.backend,
        &cancellation(),
        "approved-operation",
    )
    .expect("Core reloads the new grant");
    assert!(
        security::grant(c.store.database.connection(), grant.grant_id)
            .expect("grant")
            .expect("grant exists")
            .consumed_at
            .is_some()
    );
    assert!(
        reserve_dispatch(
            &c.authority,
            &mut c.store,
            request,
            &authority,
            &Case::input(&c.op, &c.snapshot),
            &c.backend,
            &cancellation(),
            "replay-once"
        )
        .is_err()
    );
}
