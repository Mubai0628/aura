//! What a purge deletes, table by table, children before the rows they point at.
use aura_persistence_core::StoreResult;
use rusqlite::Connection;

use super::scope::{self, Scope, json};

/// `?1` is a JSON array of IDs.
pub(super) const IN: &str = "IN (SELECT value FROM json_each(?1))";

/// One table's rows: those whose `key` is one of `ids`.
pub(super) struct Spec<'a> {
    pub table: &'static str,
    pub key: &'static str,
    pub ids: &'a [String],
    /// A further condition, checked when the row is deleted.
    pub extra: &'static str,
    /// Rows other owners may still use: one kept for them is not a reason to look again.
    pub shared: bool,
    /// Rows that point at each other: deleted in the reverse of `ids`, which lists parents first.
    pub reversed: bool,
}

const fn spec<'a>(table: &'static str, key: &'static str, ids: &'a [String]) -> Spec<'a> {
    Spec { table, key, ids, extra: "", shared: false, reversed: false }
}

/// An operation's rows, its own row last.
pub(super) const OPERATION_ROWS: [(&str, &str); 9] = [
    ("execution_operational_state", "operation_id"),
    ("execution_results", "operation_id"),
    ("execution_attempts", "operation_id"),
    ("operation_journal_entries", "operation_id"),
    ("operation_idempotency", "operation_id"),
    ("operation_leases", "operation_id"),
    ("operation_artifact_links", "operation_id"),
    ("workspace_mutation_receipts", "operation_id"),
    ("operations", "id"),
];

/// The artifacts an operation's rows hold.
pub(super) const OPERATION_ARTIFACTS: [&str; 3] = [
    "SELECT receipt_artifact_id FROM execution_results WHERE operation_id {IN}",
    "SELECT artifact_id FROM operation_artifact_links WHERE operation_id {IN}",
    "SELECT receipt_artifact_id FROM workspace_mutation_receipts WHERE operation_id {IN}",
];

/// Events and their outbox rows: an outbox row is written only with its event, so the plan
/// finds every one that points at a planned event.
pub(super) fn events(events: &[String]) -> [Spec<'_>; 2] {
    [spec("projection_outbox", "event_id", events), spec("events", "event_id", events)]
}

pub(super) fn session(scope: &Scope) -> Vec<Spec<'_>> {
    let session = std::slice::from_ref(&scope.session);
    let mut specs: Vec<Spec<'_>> = events(&scope.events).into();
    for table in [
        "agent_visible_index",
        "agent_visible_body",
        "agent_visible_delta",
        "agent_visible_coverage",
        "agent_visible_repair",
        "agent_visible_repair_turn",
    ] {
        specs.push(spec(table, "owner", &scope.owners));
    }
    specs.extend([
        spec("hook_invocations", "id", &scope.hooks),
        spec("hook_decisions", "subject", &scope.owners),
        spec("turn_poll_capture", "id", &scope.turns),
        spec("turn_poll", "id", &scope.turns),
        spec("artifact_access_links", "subject", &scope.owners),
        spec("artifact_owners", "owner", &scope.owners),
        spec("tool_calls", "batch_id", &scope.batches),
        spec("tool_batches", "id", &scope.batches),
    ]);
    specs.extend(OPERATION_ROWS.map(|(table, key)| spec(table, key, &scope.operations)));
    specs.extend([
        spec("terminal_output_segments", "terminal_id", &scope.terminals),
        spec("agent_terminals", "session_id", session),
        spec("provider_tool_call_bindings", "snapshot_id", &scope.snapshots),
        spec("transport_attempts", "snapshot_id", &scope.snapshots),
        spec("model_outputs", "snapshot_id", &scope.snapshots),
        spec("provider_continuation_refs", "session_id", session),
        spec("model_steps", "agent_run_id", &scope.runs),
        spec("recovery_episodes", "agent_run_id", &scope.runs),
        spec("model_execution_snapshots", "session_id", session),
        // `runs` lists each run before the runs it started.
        Spec { reversed: true, ..spec("agent_runs", "id", &scope.runs) },
        spec("turn_state_snapshots", "turn_task_id", &scope.turns),
        spec("turn_tasks", "id", &scope.turns),
    ]);
    for table in [
        "context_snapshots",
        "prompt_bundles",
        "context_compactions",
        "history_atomic_groups",
        "context_search_fts",
        "permission_snapshots",
        "session_model_bindings",
        "native_session_consents",
    ] {
        specs.push(spec(table, "session_id", session));
    }
    specs.extend([
        // The Session's tool registry snapshots, once nothing left uses them.
        Spec {
            extra: "AND NOT EXISTS(SELECT 1 FROM context_snapshots c
                    WHERE c.registry_snapshot_id=tool_registry_snapshots.id)",
            shared: true,
            ..spec("tool_registry_snapshots", "id", &scope.registries)
        },
        spec("authorization_grants", "id", &scope.grants),
        spec("approval_requests", "id", &scope.approvals),
        Spec { extra: "AND owner_kind='session'", ..spec("tool_pins", "owner_id", session) },
        spec("sessions", "id", session),
    ]);
    specs
}

/// Artifacts owned by the Session or referenced by the rows about to be deleted.
pub(super) fn held(db: &Connection, scope: &Scope) -> StoreResult<Vec<String>> {
    let mut held = Vec::new();
    let mut add = |sql: &str, ids: &[String]| -> StoreResult<()> {
        if !ids.is_empty() {
            held.extend(scope::column(db, &sql.replace("{IN}", IN), &json(ids))?);
        }
        Ok(())
    };
    add("SELECT artifact_id FROM artifact_owners WHERE owner {IN}", &scope.owners)?;
    for sql in OPERATION_ARTIFACTS {
        add(sql, &scope.operations)?;
    }
    add("SELECT artifact_id FROM model_outputs WHERE snapshot_id {IN}", &scope.snapshots)?;
    add(
        "SELECT artifact_id FROM terminal_output_segments WHERE terminal_id {IN}",
        &scope.terminals,
    )?;
    held.sort();
    held.dedup();
    Ok(held)
}
