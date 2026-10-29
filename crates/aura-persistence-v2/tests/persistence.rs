use aura_domain_common::{
    ArtifactId, EventId, Revision, SecurityOperationId, UtcTimestamp, WorkspaceId,
};
use aura_execution_contract::AuthorizedDispatch;
use aura_persistence_core::{
    PersistenceCoreError as Error, StoreResult, artifact, event,
    operation::{self, BeginOperation, OperationDraft, OperationState},
    schema,
};
use aura_persistence_v2::{V2Store, execution, security, workspace};
use aura_workspace_domain::{CatalogFreshness, WorkspaceState, WorkspaceSummary};
use serde_json::json;

fn now() -> UtcTimestamp {
    "2026-09-07T01:00:00Z".parse().expect("now")
}
fn workspace_fixture(id: WorkspaceId) -> WorkspaceSummary {
    WorkspaceSummary {
        workspace_id: id,
        display_name: "Test".into(),
        selected_root: "C:/synthetic".into(),
        canonical_root: "C:/synthetic".into(),
        root_identity: None,
        state: WorkspaceState::Offline,
        revision: Revision::new(1),
        catalog_freshness: CatalogFreshness::Unavailable,
        authorization_epoch: Revision::new(1),
    }
}

#[test]
fn empty_baseline_owned_schema_integrity_and_legacy_rejection() {
    let t =
        tempfile::tempdir().expect("empty baseline owned schema integrity and legacy rejection");
    let root = t.path().join("v2");
    let store =
        V2Store::create(&root).expect("empty baseline owned schema integrity and legacy rejection");
    let count:i64=store.database.connection().query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' AND name NOT GLOB '*_fts_*'",[],|r|r.get(0)).expect("empty baseline owned schema integrity and legacy rejection");
    // 65 baseline tables, two terminal tables, nine package tables
    // (B1 content and trust, B2 installed settings and HTTP credential bindings) and
    // two Hook tables (invocation records and pre-event decisions), two
    // MCP tables (discovered tool catalogs and tool contract revisions) and two
    // credential tables (MCP secrets and OAuth sign-ins).
    // The count is deliberate: it is what makes a table
    // appearing or disappearing without a reviewed predecessor visible here rather
    // than at the next user's first launch.
    assert_eq!(count, 82);
    schema::verify(store.database.connection(), aura_persistence_v2::MODULES)
        .expect("empty baseline owned schema integrity and legacy rejection");
    drop(store);
    V2Store::open(&root).expect("empty baseline owned schema integrity and legacy rejection");
    assert!(V2Store::create(&root).is_err());
    let legacy = t.path().join("old.db");
    std::fs::write(&legacy, b"untouched-legacy-canary")
        .expect("empty baseline owned schema integrity and legacy rejection");
    assert!(schema::open(&legacy, aura_persistence_v2::MODULES).is_err());
    assert_eq!(
        std::fs::read(legacy).expect("empty baseline owned schema integrity and legacy rejection"),
        b"untouched-legacy-canary"
    );
    let c = rusqlite::Connection::open(root.join("aura-v2.db"))
        .expect("empty baseline owned schema integrity and legacy rejection");
    c.execute_batch("DROP INDEX sessions_workspace")
        .expect("empty baseline owned schema integrity and legacy rejection");
    drop(c);
    assert!(V2Store::open(&root).is_err());
}

#[test]
fn state_audit_outbox_rollback_ack_and_artifact_access_are_atomic() {
    let t = tempfile::tempdir()
        .expect("state audit outbox rollback ack and artifact access are atomic");
    let root = t.path().join("v2");
    let mut s = V2Store::create(&root)
        .expect("state audit outbox rollback ack and artifact access are atomic");
    let wid = WorkspaceId::new();
    let artifact_id = ArtifactId::new();
    let write = |tx: &rusqlite::Transaction<'_>| -> StoreResult<()> {
        workspace::insert(tx, &workspace_fixture(wid))?;
        artifact::put(tx, artifact_id, "workspace-test", "text/plain", b"private-canary", now())?;
        event::append(
            tx,
            &event::EventDraft {
                id: EventId::new(),
                owner: "workspace-test",
                kind: "created",
                version: 1,
                payload: &json!({"version":1}),
                at: now(),
            },
            "workspace",
        )?;
        Ok(())
    };
    let fail: StoreResult<()> = s.database.unit_of_work(|tx| {
        write(tx)?;
        Err(Error::Invalid("before commit"))
    });
    assert!(fail.is_err());
    assert!(
        workspace::get(s.database.connection(), wid)
            .expect("state audit outbox rollback ack and artifact access are atomic")
            .is_none()
    );
    assert_eq!(event_rows(s.database.connection()), 0, "the rolled-back event is not kept");
    s.database
        .unit_of_work(write)
        .expect("state audit outbox rollback ack and artifact access are atomic");
    drop(s);
    let mut s = V2Store::open(&root)
        .expect("state audit outbox rollback ack and artifact access are atomic");
    assert_eq!(event_rows(s.database.connection()), 1, "the committed event is kept once");
    assert!(artifact::read(s.database.connection(), artifact_id, "other-session").is_err());
    s.database
        .unit_of_work(|tx| {
            artifact::grant_access(tx, artifact_id, "workspace-test", "session-test")
        })
        .expect("state audit outbox rollback ack and artifact access are atomic");
    assert_eq!(
        artifact::read(s.database.connection(), artifact_id, "session-test")
            .expect("state audit outbox rollback ack and artifact access are atomic"),
        b"private-canary"
    );
    assert!(s.database.connection().execute("DELETE FROM events", []).is_err());
    s.database
        .unit_of_work(|tx| {
            artifact::release_owner(tx, artifact_id, "workspace-test")?;
            assert_eq!(artifact::collect(tx)?, 1);
            Ok::<_, Error>(())
        })
        .expect("state audit outbox rollback ack and artifact access are atomic");
}

#[test]
fn operation_idempotency_cas_fencing_and_unknown_are_not_replayed() {
    let t = tempfile::tempdir()
        .expect("operation idempotency cas fencing and unknown are not replayed");
    let mut s = V2Store::create(&t.path().join("v2"))
        .expect("operation idempotency cas fencing and unknown are not replayed");
    let id = SecurityOperationId::new();
    let value = json!({"version":1,"path":"readme"});
    let draft = OperationDraft {
        id,
        domain: "file",
        kind: "read",
        request_key: "request-1",
        version: 1,
        payload: &value,
        at: now(),
    };
    assert_eq!(
        s.database
            .unit_of_work(|tx| operation::begin(tx, &draft))
            .expect("operation idempotency cas fencing and unknown are not replayed"),
        BeginOperation::Created
    );
    let fractional = "2026-09-07T01:00:00.001Z"
        .parse()
        .expect("operation idempotency cas fencing and unknown are not replayed");
    s.database
        .unit_of_work(|tx| operation::acquire_lease(tx, id, "fractional-owner", now(), fractional))
        .expect("operation idempotency cas fencing and unknown are not replayed");
    assert!(
        s.database
            .unit_of_work(|tx| operation::acquire_lease(tx, id, "early-steal", now(), fractional))
            .is_err()
    );
    assert!(matches!(
        s.database
            .unit_of_work(|tx| operation::begin(tx, &draft))
            .expect("operation idempotency cas fencing and unknown are not replayed"),
        BeginOperation::Existing { .. }
    ));
    let changed = OperationDraft { payload: &json!({"version":1,"path":"other"}), ..draft };
    assert!(s.database.unit_of_work(|tx| operation::begin(tx, &changed)).is_err());
    let expires = "2026-09-07T02:00:00Z"
        .parse()
        .expect("operation idempotency cas fencing and unknown are not replayed");
    let later = "2026-09-07T03:00:00Z"
        .parse()
        .expect("operation idempotency cas fencing and unknown are not replayed");
    let generation = s
        .database
        .unit_of_work(|tx| operation::acquire_lease(tx, id, "runtime-1", fractional, expires))
        .expect("operation idempotency cas fencing and unknown are not replayed");
    assert!(
        s.database
            .unit_of_work(|tx| operation::acquire_lease(tx, id, "runtime-2", now(), expires))
            .is_err()
    );
    let newer = s
        .database
        .unit_of_work(|tx| operation::acquire_lease(tx, id, "runtime-2", expires, later))
        .expect("operation idempotency cas fencing and unknown are not replayed");
    assert!(newer > generation);
    assert!(
        s.database
            .unit_of_work(|tx| operation::renew_lease(
                tx,
                id,
                "runtime-1",
                generation,
                expires,
                later
            ))
            .is_err()
    );
    s.database
        .unit_of_work(|tx| operation::transition(tx, id, 1, OperationState::Running, now()))
        .expect("operation idempotency cas fencing and unknown are not replayed");
    assert!(
        s.database
            .unit_of_work(|tx| operation::transition(tx, id, 1, OperationState::Succeeded, now()))
            .is_err()
    );
    assert_eq!(
        operation::recoverable(s.database.connection())
            .expect("operation idempotency cas fencing and unknown are not replayed"),
        vec![id]
    );
    s.database
        .unit_of_work(|tx| operation::transition(tx, id, 2, OperationState::Unknown, now()))
        .expect("operation idempotency cas fencing and unknown are not replayed");
    assert!(
        s.database
            .unit_of_work(|tx| operation::transition(tx, id, 3, OperationState::Running, now()))
            .is_err()
    );
    assert_eq!(
        s.database
            .unit_of_work(|tx| operation::begin(tx, &draft))
            .expect("operation idempotency cas fencing and unknown are not replayed"),
        BeginOperation::Existing { operation_id: id, state: OperationState::Unknown }
    );
}

#[test]
fn grant_attempt_reservation_and_unique_result_survive_restart() {
    let t =
        tempfile::tempdir().expect("grant attempt reservation and unique result survive restart");
    let root = t.path().join("v2");
    let mut s = V2Store::create(&root)
        .expect("grant attempt reservation and unique result survive restart");
    let f = aura_contract_schema::v2_kernel_fixture()
        .expect("grant attempt reservation and unique result survive restart");
    let at = f.authority.decided_at;
    let id = f.dispatch.operation.operation_id;
    let value = serde_json::to_value(&f.dispatch.operation)
        .expect("grant attempt reservation and unique result survive restart");
    s.database
        .unit_of_work(|tx| {
            operation::begin(
                tx,
                &OperationDraft {
                    id,
                    domain: "execution",
                    kind: "fixture.read",
                    request_key: "dispatch",
                    version: 1,
                    payload: &value,
                    at,
                },
            )?;
            security::insert_grant(tx, &f.grant)
        })
        .expect("grant attempt reservation and unique result survive restart");
    let dispatch = AuthorizedDispatch::revalidate(
        f.dispatch.clone(),
        &f.authority,
        &f.dispatch.plan.prepared.backend,
        &f.cancellation,
        at,
    )
    .expect("grant attempt reservation and unique result survive restart");
    assert!(
        s.database
            .unit_of_work(|tx| execution::prepare(
                tx,
                &dispatch,
                &f.authority,
                0,
                f.authority.expires_at
            ))
            .is_err()
    );
    let fail: StoreResult<()> = s.database.unit_of_work(|tx| {
        execution::prepare(tx, &dispatch, &f.authority, 0, at)?;
        Err(Error::Invalid("after authority consumed"))
    });
    assert!(fail.is_err());
    assert!(
        security::grant(s.database.connection(), f.grant.grant_id)
            .expect("grant attempt reservation and unique result survive restart")
            .expect("grant attempt reservation and unique result survive restart")
            .consumed_at
            .is_none()
    );
    s.database
        .unit_of_work(|tx| execution::prepare(tx, &dispatch, &f.authority, 0, at))
        .expect("grant attempt reservation and unique result survive restart");
    assert!(
        s.database
            .unit_of_work(|tx| execution::prepare(tx, &dispatch, &f.authority, 0, at))
            .is_err()
    );
    s.database
        .unit_of_work(|tx| execution::mark_dispatched(tx, id, 1, at))
        .expect("grant attempt reservation and unique result survive restart");
    drop(s);
    let mut s =
        V2Store::open(&root).expect("grant attempt reservation and unique result survive restart");
    assert_eq!(
        operation::recoverable(s.database.connection())
            .expect("grant attempt reservation and unique result survive restart"),
        vec![id]
    );
    s.database
        .unit_of_work(|tx| {
            artifact::put(
                tx,
                f.result.receipt.evidence_ref,
                &id.to_string(),
                "application/json",
                b"actual persisted receipt fixture",
                at,
            )?;
            execution::complete(tx, id, 1, &f.result)
        })
        .expect("grant attempt reservation and unique result survive restart");
    assert!(s.database.unit_of_work(|tx| execution::complete(tx, id, 1, &f.result)).is_err());
    s.database
        .unit_of_work(|tx| {
            artifact::release_owner(tx, f.result.receipt.evidence_ref, &id.to_string())?;
            assert_eq!(artifact::collect(tx)?, 0);
            Ok::<_, Error>(())
        })
        .expect("grant attempt reservation and unique result survive restart");
    assert_eq!(
        execution::result(s.database.connection(), id, 1)
            .expect("grant attempt reservation and unique result survive restart"),
        Some(f.result)
    );
    assert!(
        s.database
            .connection()
            .execute("UPDATE execution_results SET outcome='succeeded'", [])
            .is_err()
    );
    let execution_events: i64 = s
        .database
        .connection()
        .query_row("SELECT count(*) FROM events WHERE kind LIKE 'execution.%'", [], |r| r.get(0))
        .expect("grant attempt reservation and unique result survive restart");
    assert!(execution_events > 0);
}

#[test]
fn complex_domain_snapshots_and_history_use_exact_persisted_facts() {
    let t = tempfile::tempdir()
        .expect("complex domain snapshots and history use exact persisted facts");
    let root = t.path().join("v2");
    let mut s = V2Store::create(&root)
        .expect("complex domain snapshots and history use exact persisted facts");
    let p06 = aura_context_fixtures::p06_prepared_context_fixture()
        .expect("complex domain snapshots and history use exact persisted facts");
    let mut context = p06.prepared.context.snapshot;
    let session = context.session_id;
    let wid = WorkspaceId::new();
    let registry = aura_tool_contract::ToolRegistrySnapshotV1::new(
        context.tool_schema_set.registry_snapshot_ref.tool_registry_snapshot_id,
        Revision::new(1),
        aura_tool_contract::ToolRegistryScopeV1::Workspace { workspace_id: wid },
        None,
        None,
        aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"eligibility"),
        vec![],
        vec![],
        vec![],
        now(),
    )
    .expect("complex domain snapshots and history use exact persisted facts");
    context.tool_schema_set = registry.schema_set.clone();
    let context =
        context.seal().expect("complex domain snapshots and history use exact persisted facts");
    let model = p06.execution_snapshot;
    let hash =
        aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"persisted exact history fixture");
    let group = aura_context_domain::HistoryAtomicGroupPreimageV1 {
        kind: aura_context_domain::HistoryAtomicGroupKindV1,
        preimage_version: aura_domain_common::ToolWireVersionV1,
        session_id: session,
        turn_task_id: context.turn_task_id,
        agent_run_id: context.agent_run_id,
        model_step_id: context.model_step_id,
        assistant_response_event_id: EventId::new(),
        assistant_response_content_hash: hash.clone(),
        tool_batch_id: aura_domain_common::ToolBatchId::new(),
        calls: vec![aura_context_domain::HistoryToolCallResultEntryV1 {
            ordinal: 1,
            tool_call_id: aura_domain_common::ToolCallId::new(),
            tool_call_fact_hash: hash.clone(),
            tool_result_fact_hash: hash.clone(),
        }],
        batch_summary_hash: hash,
    };
    s.database
        .unit_of_work(|tx| {
            workspace::insert(tx, &workspace_fixture(wid))?;
            for id in std::collections::BTreeSet::from([session, group.session_id]) {
                workspace::insert_session(
                    tx,
                    &workspace::SessionMetadata {
                        id,
                        workspace_id: wid,
                        title: "Session".into(),
                        created_at: now(),
                        updated_at: now(),
                    },
                )?;
            }
            aura_persistence_v2::model::bind_session(tx, session, model.model_profile_id, None)?;
            aura_persistence_v2::model::insert_snapshot(tx, session, &model)?;
            aura_persistence_v2::tool::insert_registry(tx, &registry)?;
            aura_persistence_v2::context::insert_snapshot(tx, &context)?;
            aura_persistence_v2::context::insert_bundle(tx, session, &p06.prompt_bundle_snapshot)?;
            aura_persistence_v2::context::append_history(tx, &group)?;
            aura_persistence_v2::agent::insert_turn(
                tx,
                context.turn_task_id,
                session,
                &json!({"version":1}),
            )?;
            aura_persistence_v2::agent::insert_run(
                tx,
                context.agent_run_id,
                context.turn_task_id,
                None,
                &json!({"version":1}),
            )?;
            aura_persistence_v2::agent::update_turn(
                tx,
                context.turn_task_id,
                1,
                "running",
                &json!({"version":1,"step":1}),
            )?;
            Ok::<_, Error>(())
        })
        .expect("complex domain snapshots and history use exact persisted facts");
    drop(s);
    let mut s = V2Store::open(&root)
        .expect("complex domain snapshots and history use exact persisted facts");
    assert_eq!(
        aura_persistence_v2::context::snapshot(s.database.connection(), context.id)
            .expect("complex domain snapshots and history use exact persisted facts"),
        Some(context.clone())
    );
    assert_eq!(
        aura_persistence_v2::model::snapshot(s.database.connection(), model.id)
            .expect("complex domain snapshots and history use exact persisted facts"),
        Some(model)
    );
    assert_eq!(
        aura_persistence_v2::tool::registry(
            s.database.connection(),
            registry.snapshot_ref.tool_registry_snapshot_id
        )
        .expect("complex domain snapshots and history use exact persisted facts")
        .as_deref(),
        Some(&registry)
    );
    assert_eq!(
        aura_persistence_v2::context::history(s.database.connection(), group.session_id)
            .expect("complex domain snapshots and history use exact persisted facts"),
        vec![group.clone()]
    );
    s.database
        .unit_of_work(|tx| aura_persistence_v2::context::append_history(tx, &group))
        .expect("exact committed history replay is idempotent after restart");
    let mut changed = group.clone();
    changed.calls[0].tool_result_fact_hash =
        aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"different result");
    assert!(
        s.database
            .unit_of_work(|tx| aura_persistence_v2::context::append_history(tx, &changed))
            .is_err(),
        "the same batch cannot replace its committed result"
    );
    assert_eq!(
        aura_persistence_v2::context::history(s.database.connection(), group.session_id)
            .expect("history after exact replay and rejected changed result"),
        vec![group],
        "neither exact replay nor conflicting input appends another history group"
    );
    assert!(
        s.database
            .unit_of_work(|tx| aura_persistence_v2::agent::update_turn(
                tx,
                context.turn_task_id,
                1,
                "done",
                &json!({"version":1})
            ))
            .is_err()
    );
}

/// Audit events in the store; each is written once.
fn event_rows(connection: &rusqlite::Connection) -> i64 {
    connection
        .query_row("SELECT count(*) FROM events", [], |row| row.get(0))
        .expect("count the audit events")
}
