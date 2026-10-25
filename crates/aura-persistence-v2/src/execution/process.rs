//! The process page consumes existing Shell observations; no new process owner.
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use aura_windows_user_contract::*;
use rusqlite::Connection;
pub fn running_shells(
    db: &Connection,
    session: SessionId,
    live: &[SecurityOperationId],
) -> StoreResult<Vec<ManagedProcessViewV1>> {
    let mut statement = db.prepare("SELECT c.id,o.id FROM tool_calls c JOIN operations o ON o.id=c.operation_id WHERE json_extract(o.payload,'$.content.identity.session_id')=?1 AND json_extract(o.payload,'$.content.action_kind') IN ('shell.cmd','shell.powershell') ORDER BY c.rowid")?;
    let ids = statement
        .query_map([session.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = Vec::new();
    for (id, operation) in ids {
        let operation =
            operation.parse().map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)?;
        if !live.contains(&operation) {
            continue;
        }
        let call = id.parse().map_err(|_| aura_persistence_core::PersistenceCoreError::Conflict)?;
        let view = super::shell::query(
            db,
            session,
            &ShellQueryInput {
                tool_call_id: call,
                stdout_offset: 0,
                stderr_offset: 0,
                max_bytes: 1,
            },
        )?;
        if view.background == Some(BackgroundState::Running)
            && let Some(started_at) = view.started_at
        {
            let mut command: String = view.original_input.script.chars().take(4096).collect();
            if command.len() < view.original_input.script.len() {
                command.push('…');
            }
            result.push(ManagedProcessViewV1 {
                kind: ManagedProcessKindV1::Shell,
                command,
                started_at,
                tool_call_id: call,
                terminal_id: None,
            });
        }
    }
    Ok(result)
}
