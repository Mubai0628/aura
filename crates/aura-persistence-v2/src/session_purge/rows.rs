//! Deletes a Session's rows, children before the rows they point at. Foreign keys stay enforced,
//! so a row this list forgot fails the purge instead of leaving a dangling reference behind.
use aura_persistence_core::StoreResult;
use rusqlite::{Transaction, params};

use super::PurgeReport;
use super::scope::{Scope, json};

/// `?1` is a JSON array of IDs.
pub(super) const IN: &str = "IN (SELECT value FROM json_each(?1))";

/// An operation's rows, its own row last.
pub(super) const OPERATION_ROWS: [&str; 9] = [
    "DELETE FROM execution_operational_state WHERE operation_id {IN}",
    "DELETE FROM execution_results WHERE operation_id {IN}",
    "DELETE FROM execution_attempts WHERE operation_id {IN}",
    "DELETE FROM operation_journal_entries WHERE operation_id {IN}",
    "DELETE FROM operation_idempotency WHERE operation_id {IN}",
    "DELETE FROM operation_leases WHERE operation_id {IN}",
    "DELETE FROM operation_artifact_links WHERE operation_id {IN}",
    "DELETE FROM workspace_mutation_receipts WHERE operation_id {IN}",
    "DELETE FROM operations WHERE id {IN}",
];

/// The artifacts an operation's rows hold.
pub(super) const OPERATION_ARTIFACTS: [&str; 3] = [
    "SELECT receipt_artifact_id FROM execution_results WHERE operation_id {IN}",
    "SELECT artifact_id FROM operation_artifact_links WHERE operation_id {IN}",
    "SELECT receipt_artifact_id FROM workspace_mutation_receipts WHERE operation_id {IN}",
];

pub(super) fn delete(tx: &Transaction<'_>, scope: &Scope) -> StoreResult<PurgeReport> {
    let mut rows = 0;
    let mut by = |sql: &str, ids: &[String]| -> StoreResult<()> {
        if !ids.is_empty() {
            rows += tx.execute(&sql.replace("{IN}", IN), [json(ids)])?;
        }
        Ok(())
    };
    let session = std::slice::from_ref(&scope.session);

    // The artifacts these rows hold, released once the rows are gone.
    let held = held_artifacts(tx, scope)?;

    // Events and what was derived from them.
    by("DELETE FROM projection_outbox WHERE event_id {IN}", &scope.events)?;
    by("DELETE FROM events WHERE event_id {IN}", &scope.events)?;
    for table in [
        "agent_visible_index",
        "agent_visible_body",
        "agent_visible_delta",
        "agent_visible_coverage",
        "agent_visible_repair",
        "agent_visible_repair_turn",
    ] {
        by(&format!("DELETE FROM {table} WHERE owner {{IN}}"), &scope.owners)?;
    }
    by("DELETE FROM hook_invocations WHERE id {IN}", &scope.hooks)?;
    by("DELETE FROM hook_decisions WHERE subject {IN}", &scope.owners)?;
    by("DELETE FROM turn_poll_capture WHERE id {IN}", &scope.turns)?;
    by("DELETE FROM turn_poll WHERE id {IN}", &scope.turns)?;
    by("DELETE FROM artifact_access_links WHERE subject {IN}", &scope.owners)?;
    by("DELETE FROM artifact_owners WHERE owner {IN}", &scope.owners)?;

    // Tool calls and the operations they ran.
    by("DELETE FROM tool_calls WHERE batch_id {IN}", &scope.batches)?;
    by("DELETE FROM tool_batches WHERE id {IN}", &scope.batches)?;
    for sql in OPERATION_ROWS {
        by(sql, &scope.operations)?;
    }

    // Terminals.
    by("DELETE FROM terminal_output_segments WHERE terminal_id {IN}", &scope.terminals)?;
    by("DELETE FROM agent_terminals WHERE session_id {IN}", session)?;

    // Model executions and the turns that made them.
    for table in ["provider_tool_call_bindings", "transport_attempts", "model_outputs"] {
        by(&format!("DELETE FROM {table} WHERE snapshot_id {{IN}}"), &scope.snapshots)?;
    }
    by("DELETE FROM provider_continuation_refs WHERE session_id {IN}", session)?;
    by("DELETE FROM model_steps WHERE agent_run_id {IN}", &scope.runs)?;
    by("DELETE FROM recovery_episodes WHERE agent_run_id {IN}", &scope.runs)?;
    by("DELETE FROM model_execution_snapshots WHERE session_id {IN}", session)?;
    by("DELETE FROM agent_runs WHERE id {IN}", &scope.runs)?;
    by("DELETE FROM turn_state_snapshots WHERE turn_task_id {IN}", &scope.turns)?;
    by("DELETE FROM turn_tasks WHERE id {IN}", &scope.turns)?;

    // Context, permission and model selection.
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
        by(&format!("DELETE FROM {table} WHERE session_id {{IN}}"), session)?;
    }
    // The Session's tool registry snapshots, once nothing left points at them.
    by(
        "DELETE FROM tool_registry_snapshots WHERE id {IN}
         AND NOT EXISTS(SELECT 1 FROM tool_batches b WHERE b.registry_snapshot_id=tool_registry_snapshots.id)
         AND NOT EXISTS(SELECT 1 FROM context_snapshots c WHERE c.registry_snapshot_id=tool_registry_snapshots.id)",
        &scope.registries,
    )?;
    by("DELETE FROM authorization_grants WHERE id {IN}", &scope.grants)?;
    by("DELETE FROM approval_requests WHERE id {IN}", &scope.approvals)?;
    rows += tx.execute(
        "DELETE FROM tool_pins WHERE owner_kind='session' AND owner_id=?1",
        [&scope.session],
    )?;
    rows += tx.execute("DELETE FROM sessions WHERE id=?1", [&scope.session])?;

    let artifacts = release(tx, &held)?;
    Ok(PurgeReport { rows, artifacts })
}

/// Artifacts owned by the Session or referenced by the rows about to be deleted.
fn held_artifacts(tx: &Transaction<'_>, scope: &Scope) -> StoreResult<Vec<String>> {
    let mut held = Vec::new();
    let mut add = |sql: &str, ids: &[String]| -> StoreResult<()> {
        if !ids.is_empty() {
            held.extend(super::scope::column(tx, &sql.replace("{IN}", IN), &json(ids))?);
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

/// Queues the held artifacts nothing else owns and collects them with the objects only they held.
pub(super) fn release(tx: &Transaction<'_>, held: &[String]) -> StoreResult<usize> {
    for id in held {
        tx.execute(
            "INSERT INTO artifact_gc_queue SELECT id FROM artifacts
             WHERE id=?1 AND NOT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=?1)
             ON CONFLICT DO NOTHING",
            params![id],
        )?;
    }
    aura_persistence_core::artifact::collect(tx)
}
