//! Carry existing instance-owned Agent configuration to verified revisions in
//! the credential authority. The HTTP credential owner is always materialized here.
use super::*;
mod carry;
mod projections;
pub(crate) use carry::{clear_rebind_reason, rebind_reason};
#[cfg(test)]
mod fs_followups_tests;
#[cfg(test)]
mod tests;

impl PersistenceCoordinator {
    pub fn builtin_credential_rebind_reason(
        &self,
        id: ToolInstanceId,
    ) -> Result<Option<String>, PersistenceError> {
        self.with_reader(move |db| rebind_reason(db, id))
    }
    pub(super) fn materialize_http_credential_target(
        &self,
        now: UtcTimestamp,
    ) -> Result<Option<(Revision, Vec<ProjectionChange>)>, PersistenceError> {
        let release = aura_tool_registry::agent_release::release()
            .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
        let entries = release
            .manifest()
            .entries
            .iter()
            .filter(|entry| {
                !matches!(
                    entry.handler,
                    aura_tool_registry::agent_release::AgentBuiltinHandler::Registry(_)
                )
            })
            .cloned()
            .collect();
        self.materialize_agent_builtin_configuration(entries, now)
    }

    pub(super) fn materialize_agent_builtin_configuration(
        &self,
        entries: Vec<aura_tool_registry::agent_release::AgentBuiltinEntry>,
        now: UtcTimestamp,
    ) -> Result<Option<(Revision, Vec<ProjectionChange>)>, PersistenceError> {
        let runtime = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let tx = connection.transaction()?;
            let mut changed = BTreeSet::new();
            for entry in &entries {
            let previous = carry::previous(&tx,entry)?;
            // Agent settings are ToolId-owned. Only existing instance-owned configuration
            // and the HTTP credential owner need materialization in this authority.
            if previous.is_none() && entry.handler.name() != "http.request" { continue; }
            let definition = crate::tool_entities::load_optional_record::<ToolDefinitionRecordV1, _>(&tx,
                "SELECT record_json, record_hash_json FROM tool_definitions WHERE tool_id=?1 AND tool_version_json=?2",
                params![entry.definition.tool_id.to_string(),encode_json(&entry.definition.tool_version)?])?;
            let instance = crate::tool_entities::load_optional_record::<ToolInstanceRecordV1, _>(&tx,
                "SELECT record_json, record_hash_json FROM tool_instances WHERE tool_instance_id=?1",
                [entry.instance.tool_instance_id.to_string()])?;
            if definition.as_ref().is_some_and(|stored| stored != &entry.definition)
                || instance.as_ref().is_some_and(|stored| !carry::same_identity(stored,&entry.instance)) {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            if definition.is_some() && instance.is_some() { continue; }
            if definition.is_none() { insert_definition(&tx,&entry.definition)?; }
            if instance.is_none() {
                let mut incoming = entry.instance.clone();
                if let Some(previous) = previous.as_ref() {
                    incoming.enabled = previous.enabled;
                    incoming.overrides = previous.overrides.clone();
                    incoming.runtime_health = previous.runtime_health;
                }
                insert_instance(&tx,&incoming)?;
                if let Some(previous) = previous { carry::configuration(&tx,&previous,entry,now)?; }
            }
            changed.insert(entry.definition.tool_id);
            }
            if changed.is_empty() { return Ok(None); }
            let revision = advance_registry_revision(&tx,now)?;
            let drafts = tool_definition_projection_drafts(&tx,Some(&changed))?;
            let (changes, _) = insert_tool_projections(&tx,runtime,drafts)?;
            tx.commit()?;
            Ok(Some((revision,changes)))
        })
    }
}

pub(crate) fn credential_slot_binding_summary(
    tool: &CanonicalToolPackageToolV1,
    binding: Option<&ToolInstanceCredentialBindingRecordV1>,
) -> Option<ToolCredentialSlotBindingSummaryV1> {
    credential_slot_binding_summary_for_runtime(&tool.runtime.to_runtime_descriptor_v1(), binding)
}

pub(crate) fn credential_slot_binding_summary_for_runtime(
    runtime: &ToolRuntimeDescriptorV1,
    binding: Option<&ToolInstanceCredentialBindingRecordV1>,
) -> Option<ToolCredentialSlotBindingSummaryV1> {
    let (slot_key, placement) = crate::tool_credential::exact_runtime_slot(runtime)?;
    let bound_credential = binding
        .filter(|binding| binding.credential_slot_key == slot_key && binding.placement == placement)
        .map(|binding| ToolBoundCredentialSummaryV1 {
            credential_id: binding.credential_id,
            credential_revision: binding.credential_revision,
            binding_revision: binding.binding_revision,
        });
    Some(ToolCredentialSlotBindingSummaryV1 {
        slot_key,
        placement,
        bound_credential,
        rebind_required_reason: None,
    })
}

pub(super) fn projected_binding(
    db: &rusqlite::Connection,
    id: ToolInstanceId,
    mut summary: Option<ToolCredentialSlotBindingSummaryV1>,
) -> Result<Option<ToolCredentialSlotBindingSummaryV1>, PersistenceError> {
    if let Some(summary) = summary.as_mut()
        && summary.bound_credential.is_none()
    {
        summary.rebind_required_reason = rebind_reason(db, id)?;
    }
    Ok(summary)
}
