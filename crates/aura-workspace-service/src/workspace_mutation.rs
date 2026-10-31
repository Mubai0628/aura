//! What the workspace and security services agree on about a workspace mutation: which Main
//! methods mutate a workspace, so the security service gates them, and the code an external
//! target held by another execution answers with.
use aura_ipc::method;

/// The typed code for "another execution holds this external target **right now**".
///
/// It is deliberately distinct from the non-retryable `file.external_resource_held`: this one means
/// no byte was published, the holder is a live execution that will finish, and the object may still
/// be exactly what the caller decided about — so the caller's own asynchronous owner may wait within
/// its original budget and re-enter. `p03`'s dispatcher recognises exactly this code the same way it
/// recognises `fs.resource_busy` for the in-workspace path.
pub const EXTERNAL_RESOURCE_BUSY_CODE: &str = "file.external_resource_busy";

#[must_use]
pub fn gates_authenticated_main_workspace_mutation(method_name: &str) -> bool {
    matches!(
        method_name,
        method::BUFFER_SAVE
            | method::FS_WRITE_FILE
            | method::FS_APPLY_PATCH
            | method::FS_CREATE_DIRECTORY
            | method::FS_COPY
            | method::FS_MOVE
            | method::FS_DELETE
            | method::FS_APPLY_CHANGESET
    )
}
