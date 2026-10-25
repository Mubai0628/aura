use super::*;

pub(super) fn status(
    key: &BufferKey,
    record: &BufferRecord,
) -> Result<LogicalFileStatus, WorkspaceError> {
    Ok(LogicalFileStatus {
        approval_follow_up: Vec::new(),
        session_id: key.session_id,
        path: record.path.clone(),
        logical_hash: logical_hash(record)?,
        disk_hash: Some(record.disk_hash.clone()),
        buffer_revision: record.revision,
        dirty: record.dirty,
        active_editing: record.active_editing,
        ime_composition_active: record.ime_composition_active,
        reservation_id: record.reservation_id,
        reservation_epoch: record.reservation_epoch,
        core_acknowledged_revision: record.revision,
        save_state: record.save_state.clone(),
    })
}

pub(super) fn lifecycle_state(
    key: &BufferKey,
    record: &BufferRecord,
) -> Result<EditorBufferLifecycleState, WorkspaceError> {
    Ok(EditorBufferLifecycleState {
        session_id: key.session_id,
        canonical_relative_path: record.path.clone(),
        reservation_id: record.reservation_id,
        reservation_epoch: record.reservation_epoch,
        buffer_revision: record.revision,
        core_acknowledged_revision: record.revision,
        logical_hash: logical_hash(record)?,
        disk_hash: Some(record.disk_hash.clone()),
        is_dirty: record.dirty,
        is_active_editing: record.active_editing,
        ime_composition_active: record.ime_composition_active,
        save_state: record.save_state.clone(),
    })
}
