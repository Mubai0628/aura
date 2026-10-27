use super::super::{AgentBuiltinEntry, BuiltinToolDefinitionSetError};
use super::{PUBLISHED, PUBLISHED_BROWSER, PUBLISHED_LATER, PUBLISHED_MEDIA, PublishedRevision};
use aura_domain_common::{Revision, ToolId, ToolInstanceId};

pub fn current(name: &str) -> Result<&'static PublishedRevision, BuiltinToolDefinitionSetError> {
    PUBLISHED
        .iter()
        .chain(PUBLISHED_BROWSER)
        .chain(PUBLISHED_MEDIA)
        .chain(PUBLISHED_LATER)
        .filter(|r| r.name == name)
        .max_by_key(|r| r.revision)
        .ok_or(BuiltinToolDefinitionSetError::InvalidDocument)
}
pub(in crate::agent_release) fn identity(
    name: &str,
) -> Result<(ToolId, Revision, ToolInstanceId), BuiltinToolDefinitionSetError> {
    let row = current(name)?;
    Ok((
        row.tool.parse().map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
        Revision::new(row.revision),
        row.instance.parse().map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
    ))
}
pub(in crate::agent_release) fn verify(
    entries: &[AgentBuiltinEntry],
) -> Result<(), BuiltinToolDefinitionSetError> {
    for entry in entries {
        entry
            .definition
            .validate()
            .map_err(|_| BuiltinToolDefinitionSetError::DefinitionMismatch)?;
        let row = current(entry.handler.name())?;
        if row.tool != entry.definition.tool_id.to_string()
            || row.instance != entry.instance.tool_instance_id.to_string()
            || row.revision != entry.definition.tool_version.definition_revision.value()
            || row.fingerprint != entry.definition.definition_fingerprint.hex()
        {
            return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
        }
    }
    Ok(())
}
