use aura_domain_common::{CorrelationId, RuntimeInstanceId, UtcTimestamp};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_tool_registry::embedded_builtin_tool_definition_set_v1;
use rusqlite::{Connection, params};

fn start(root: &std::path::Path) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        root.join("aura.db"),
        root.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("start signed Built-in persistence")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected signed Built-in recovery: {}", value.reason)
        }
    }
}

fn scalar(connection: &Connection, sql: &str) -> i64 {
    connection.query_row(sql, [], |row| row.get(0)).expect("read signed Built-in count")
}

#[test]
fn clean_roots_share_fixed_instances_and_restart_materialization_is_zero_write() {
    let first_root = tempfile::tempdir().expect("first clean data root");
    let second_root = tempfile::tempdir().expect("second clean data root");
    let release = embedded_builtin_tool_definition_set_v1().expect("signed Built-in release");
    let expected_ids =
        release.entries().iter().map(|entry| entry.tool_instance_id).collect::<Vec<_>>();

    let first = start(first_root.path());
    let first_commit = first
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("materialize first clean root");
    assert!(first_commit.changed);
    assert_eq!(first_commit.eligible_instance_ids, expected_ids);
    assert!(first_commit.quarantined_instance_ids.is_empty());
    assert!(first_commit.event_id.is_some());

    let second = start(second_root.path());
    let second_commit = second
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("materialize second clean root");
    assert!(second_commit.changed);
    assert_eq!(second_commit.eligible_instance_ids, expected_ids);
    assert_eq!(second_commit.signed_release_hash, first_commit.signed_release_hash);

    let same_process = first
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("repeat exact release in the first process");
    assert!(!same_process.changed);
    assert_eq!(same_process.registry_revision, first_commit.registry_revision);
    assert!(same_process.event_id.is_none());
    assert!(same_process.projection_changes.is_empty());

    let before = Connection::open(first_root.path().join("aura.db")).expect("open first database");
    let event_count = scalar(
        &before,
        "SELECT COUNT(*) FROM events WHERE event_type='SignedBuiltinReleaseMaterialized'",
    );
    let snapshot_count = scalar(&before, "SELECT COUNT(*) FROM tool_registry_snapshots");
    let outbox_count = scalar(&before, "SELECT COUNT(*) FROM projection_outbox");
    drop(before);
    drop(first);

    let restarted = start(first_root.path());
    let after_restart = restarted
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("repeat exact release after Core restart");
    assert!(!after_restart.changed);
    assert_eq!(after_restart.registry_revision, first_commit.registry_revision);
    assert!(after_restart.event_id.is_none());
    assert!(after_restart.projection_changes.is_empty());
    let after = Connection::open(first_root.path().join("aura.db")).expect("reopen database");
    assert_eq!(
        scalar(
            &after,
            "SELECT COUNT(*) FROM events WHERE event_type='SignedBuiltinReleaseMaterialized'",
        ),
        event_count
    );
    assert_eq!(scalar(&after, "SELECT COUNT(*) FROM tool_registry_snapshots"), snapshot_count);
    assert_eq!(scalar(&after, "SELECT COUNT(*) FROM projection_outbox"), outbox_count);
}

#[test]
fn one_conflicting_handler_binding_quarantines_only_its_fixed_instance() {
    let root = tempfile::tempdir().expect("conflicting signed Built-in root");
    let persistence = start(root.path());
    let first = persistence
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("materialize signed Built-in release");
    assert_eq!(first.eligible_instance_ids.len(), 6, "registry tools plus the activity query");
    let affected = persistence
        .validated_signed_builtin_release_authority()
        .expect("read original signed authority")
        .into_iter()
        .find(|entry| {
            entry.execution_binding.builtin_handler()
                == aura_tool_registry::BuiltinCoreHandlerV1::ToolsSearch
        })
        .expect("search handler is initially eligible")
        .instance
        .tool_instance_id;
    let unaffected: std::collections::BTreeSet<_> =
        first.eligible_instance_ids.iter().copied().filter(|id| *id != affected).collect();
    let connection = Connection::open(root.path().join("aura.db")).expect("open Built-in database");
    assert_eq!(
        connection
            .execute(
                "UPDATE tool_instance_execution_bindings SET binding_json='{}'
                 WHERE tool_instance_id=?1",
                params![affected.to_string()],
            )
            .expect("tamper one handler binding"),
        1
    );
    drop(connection);

    let repeated = persistence
        .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
        .expect("validate exact persisted signed release");
    assert!(!repeated.changed);
    assert!(repeated.event_id.is_none());
    assert!(repeated.projection_changes.is_empty());
    assert_eq!(repeated.quarantined_instance_ids, vec![affected]);
    assert_eq!(
        repeated.eligible_instance_ids.iter().copied().collect::<std::collections::BTreeSet<_>>(),
        unaffected,
        "quarantine must retain every other fixed instance, including the activity query"
    );
    assert!(!repeated.eligible_instance_ids.contains(&affected));

    let authority = persistence
        .validated_signed_builtin_release_authority()
        .expect("read fail-closed signed Built-in authority");
    assert_eq!(
        authority
            .iter()
            .map(|entry| entry.instance.tool_instance_id)
            .collect::<std::collections::BTreeSet<_>>(),
        unaffected
    );
    assert!(authority.iter().all(|entry| entry.instance.tool_instance_id != affected));
    let connection = Connection::open(root.path().join("aura.db")).expect("reopen database");
    let retained: String = connection
        .query_row(
            "SELECT binding_json FROM tool_instance_execution_bindings
             WHERE tool_instance_id=?1",
            params![affected.to_string()],
            |row| row.get(0),
        )
        .expect("read quarantined binding");
    assert_eq!(retained, "{}", "identity conflicts are never randomly rebuilt");
}
