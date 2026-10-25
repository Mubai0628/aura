//! Legacy execution APIs require an exact definition and fail closed for unbound names.
use crate::PersistenceError;
use aura_tool_contract::{NormalizedToolCallV1, ToolDefinitionSnapshotRef};

pub(crate) fn resolved(
    call: &NormalizedToolCallV1,
) -> Result<&ToolDefinitionSnapshotRef, PersistenceError> {
    call.definition_ref.as_ref().ok_or_else(|| {
        PersistenceError::InvalidRequest(
            "unresolved Provider tool name has no executable definition".into(),
        )
    })
}
