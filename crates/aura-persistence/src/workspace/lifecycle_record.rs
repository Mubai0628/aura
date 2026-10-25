use super::*;

pub(super) fn load_lifecycle_record(
    connection: &rusqlite::Connection,
    operation_id: WorkspaceLifecycleOperationId,
) -> Result<WorkspaceLifecycleRecord, PersistenceError> {
    let row = connection
        .query_row(
            "SELECT workspace_id, renderer_instance_id, operation_kind, operation_json,
                    state, lifecycle_revision, ack_deadline_at, preflight_json, error_code
             FROM workspace_lifecycle_operations WHERE operation_id=?1",
            [operation_id.to_string()],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| {
            PersistenceError::InvalidRequest("workspace lifecycle operation not found".to_owned())
        })?;
    let operation: WorkspaceLifecycleOperation = serde_json::from_str(&row.3)?;
    let operation_kind = parse_lifecycle_operation_kind(&row.2)?;
    if operation.kind() != operation_kind {
        return Err(PersistenceError::InvalidRequest(
            "persisted lifecycle operation kind does not match its payload".to_owned(),
        ));
    }
    let ack_deadline_at = row
        .6
        .map(|value| {
            UtcTimestamp::from_str(&value).map_err(|error| {
                PersistenceError::InvalidRequest(format!(
                    "persisted lifecycle ACK deadline is invalid: {error}"
                ))
            })
        })
        .transpose()?;
    let preflight = row
        .7
        .map(|value| serde_json::from_str::<WorkspaceLifecyclePreflight>(&value))
        .transpose()?;
    Ok(WorkspaceLifecycleRecord {
        summary: WorkspaceLifecycleSummary {
            approval_follow_up: Vec::new(),
            operation_id,
            workspace_id: WorkspaceId::from_str(&row.0)?,
            renderer_instance_id: RendererInstanceId::from_str(&row.1)?,
            operation: operation_kind,
            lifecycle_revision: Revision::new(parse_sql_u64(row.5)?),
            state: parse_lifecycle_state(&row.4)?,
            ack_deadline_at,
            preflight,
            error_code: row.8,
        },
        operation,
    })
}
