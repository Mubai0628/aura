//! Everything a Session owns in the execution store, found from the Session outward.
//!
//! Rows are linked by their own columns (turns, runs, steps, model executions, tool batches and
//! calls, terminals, context and permission snapshots). Operations, approvals and grants are the
//! Session's when they name the Session or one of its operations or turns. Events are the
//! Session's when an owner above owns them, when a command, lineage or message event names the
//! Session, or when an event of the Session owns them.
use std::collections::BTreeSet;

use aura_persistence_core::StoreResult;
use rusqlite::{Connection, ToSql};

pub(super) struct Scope {
    pub session: String,
    pub turns: Vec<String>,
    pub runs: Vec<String>,
    pub snapshots: Vec<String>,
    pub batches: Vec<String>,
    pub operations: Vec<String>,
    pub approvals: Vec<String>,
    pub grants: Vec<String>,
    pub terminals: Vec<String>,
    pub hooks: Vec<String>,
    /// Tool registry snapshots, made for one of the Session's runs (their scope names it).
    pub registries: Vec<String>,
    /// Every owner string the Session's events, visible text and artifacts are filed under.
    pub owners: Vec<String>,
    pub events: Vec<String>,
}

pub(super) fn json(ids: &[String]) -> String {
    serde_json::to_string(ids).unwrap_or_else(|_| "[]".into())
}

pub(super) fn column(db: &Connection, sql: &str, param: &dyn ToSql) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(sql)?;
    let values = statement.query_map([param], |row| row.get::<_, String>(0))?;
    Ok(values.collect::<Result<_, _>>()?)
}

fn within(db: &Connection, sql: &str, ids: &[String]) -> StoreResult<Vec<String>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    column(db, sql, &json(ids))
}

pub(super) fn of_session(db: &Connection, session: &str) -> StoreResult<Scope> {
    let s = &session.to_owned();
    let turns = column(db, "SELECT id FROM turn_tasks WHERE session_id=?1", s)?;
    let mut runs = within(
        db,
        "SELECT id FROM agent_runs WHERE turn_task_id IN (SELECT value FROM json_each(?1))",
        &turns,
    )?;
    loop {
        let children = within(
            db,
            "SELECT id FROM agent_runs WHERE parent_id IN (SELECT value FROM json_each(?1))",
            &runs,
        )?;
        let before = runs.len();
        merge(&mut runs, children);
        if runs.len() == before {
            break;
        }
    }
    let steps = within(
        db,
        "SELECT id FROM model_steps WHERE agent_run_id IN (SELECT value FROM json_each(?1))",
        &runs,
    )?;
    let snapshots = column(db, "SELECT id FROM model_execution_snapshots WHERE session_id=?1", s)?;
    let batches = column(db, "SELECT id FROM tool_batches WHERE session_id=?1", s)?;
    let calls = within(
        db,
        "SELECT id FROM tool_calls WHERE batch_id IN (SELECT value FROM json_each(?1))",
        &batches,
    )?;
    let mut operations = within(
        db,
        "SELECT operation_id FROM tool_calls
         WHERE batch_id IN (SELECT value FROM json_each(?1)) AND operation_id IS NOT NULL",
        &batches,
    )?;
    merge(
        &mut operations,
        column(db, "SELECT id FROM operations WHERE instr(payload, ?1) > 0", s)?,
    );
    let terminals = column(db, "SELECT id FROM agent_terminals WHERE session_id=?1", s)?;
    let hooks = within(
        db,
        "SELECT id FROM hook_invocations WHERE turn_task_id IN (SELECT value FROM json_each(?1))",
        &turns,
    )?;
    let registries = column(
        db,
        "SELECT registry_snapshot_id FROM tool_batches WHERE session_id=?1
         UNION SELECT registry_snapshot_id FROM context_snapshots
         WHERE session_id=?1 AND registry_snapshot_id IS NOT NULL",
        s,
    )?;
    let mut named: BTreeSet<String> = [s.clone()].into();
    named.extend(turns.iter().cloned());
    named.extend(runs.iter().cloned());
    named.extend(operations.iter().cloned());
    let approvals = naming(db, "SELECT id, payload FROM approval_requests", &named, false)?;
    // A grant for the whole Workspace serves its other Sessions too; it goes with the Workspace.
    let mut grants = naming(db, "SELECT id, payload FROM authorization_grants", &named, true)?;
    merge(
        &mut grants,
        column(db, "SELECT grant_id FROM native_session_consents WHERE session_id=?1", s)?,
    );

    let mut owners: BTreeSet<String> =
        // `agent-branch` holds a branch's record and the user messages it carried.
        ["agent-inbox", "agent-user", "agent-session", "agent-tools", "agent-branch"]
            .iter()
            .map(|kind| format!("{kind}:{s}"))
            .collect();
    owners.insert(s.clone());
    for ids in [&turns, &runs, &steps, &snapshots, &batches, &calls, &operations, &approvals]
        .into_iter()
        .chain([&grants, &terminals, &hooks, &registries])
    {
        owners.extend(ids.iter().cloned());
    }
    for table in
        ["prompt_bundles", "context_snapshots", "context_compactions", "permission_snapshots"]
    {
        owners.extend(column(db, &format!("SELECT id FROM {table} WHERE session_id=?1"), s)?);
    }
    // Owners that carry the Session in their own name: `ses_…:trn_…:agt_…`.
    owners.extend(range_owners(db, &format!("{s}:"), &format!("{s};"), None)?);
    // Commands, lineages and message events that name the Session in their payload.
    for (low, high) in [("civ_", "civ`"), ("lin_", "lin`"), ("evt_", "evt`")] {
        owners.extend(range_owners(db, low, high, Some(s))?);
    }
    // Events owned by an event of the Session (visible text chunks, tool call indexes).
    let mut events: Vec<String>;
    loop {
        let list: Vec<String> = owners.iter().cloned().collect();
        events = within(
            db,
            "SELECT event_id FROM events WHERE owner IN (SELECT value FROM json_each(?1))",
            &list,
        )?;
        let before = owners.len();
        owners.extend(events.iter().cloned());
        if owners.len() == before {
            break;
        }
    }
    Ok(Scope {
        session: s.clone(),
        turns,
        runs,
        snapshots,
        batches,
        operations,
        approvals,
        grants,
        terminals,
        hooks,
        registries,
        owners: owners.into_iter().collect(),
        events,
    })
}

fn merge(into: &mut Vec<String>, more: Vec<String>) {
    let mut seen: BTreeSet<String> = into.iter().cloned().collect();
    for value in more {
        if seen.insert(value.clone()) {
            into.push(value);
        }
    }
}

/// Event owners in `[low, high)`, an index range; with `session`, only those whose events name it.
fn range_owners(
    db: &Connection,
    low: &str,
    high: &str,
    session: Option<&String>,
) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(
        "SELECT DISTINCT owner FROM events
         WHERE owner >= ?1 AND owner < ?2 AND (?3 IS NULL OR instr(payload, ?3) > 0)",
    )?;
    let values = statement.query_map(rusqlite::params![low, high, session], |row| row.get(0))?;
    Ok(values.collect::<Result<_, _>>()?)
}

/// The rows of `sql` (id, payload) whose payload names one of `named`; with `keep_workspace`,
/// not those whose lifetime is a whole Workspace.
fn naming(
    db: &Connection,
    sql: &str,
    named: &BTreeSet<String>,
    keep_workspace: bool,
) -> StoreResult<Vec<String>> {
    let mut statement = db.prepare(sql)?;
    let rows =
        statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
    let mut found = Vec::new();
    for row in rows {
        let (id, payload) = row?;
        if !ids_in(&payload).any(|id| named.contains(id)) {
            continue;
        }
        if keep_workspace && workspace_lifetime(&payload) {
            continue;
        }
        found.push(id);
    }
    Ok(found)
}

pub(super) fn workspace_lifetime(payload: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(payload)
        .is_ok_and(|value| value["scope"]["lifetime"]["lifetime"] == "workspace")
}

/// Aura IDs in a text: three lowercase letters, `_`, and a 26-character ULID.
pub(super) fn ids_in(text: &str) -> impl Iterator<Item = &str> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(29)).filter_map(move |start| {
        let candidate = &bytes[start..start + 30];
        let prefix = candidate[..3].iter().all(u8::is_ascii_lowercase) && candidate[3] == b'_';
        let body = candidate[4..].iter().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase());
        let boundary = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        (prefix && body && boundary).then(|| &text[start..start + 30])
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_aura_ids_in_a_payload() {
        let text = r#"{"lifetime":{"operation_id":"sop_01M57QWS807T1HRJTFBTQ11VEQ"},"session_id":"ses_01M57QWP913CPEHR8HR2YAAF04","x":"xsop_01M57QWS807T1HRJTFBTQ11VEQ"}"#;
        let found: Vec<&str> = super::ids_in(text).collect();
        assert_eq!(found, ["sop_01M57QWS807T1HRJTFBTQ11VEQ", "ses_01M57QWP913CPEHR8HR2YAAF04"]);
    }
}
