use super::*;
use aura_projection_contract::{ProjectionKey, ProjectionKind, ProjectionScope};

impl PersistenceCoordinator {
    /// Refresh runtime-owned views separately from zero-write release/configuration materialization.
    pub fn rebuild_agent_builtin_configuration_projections(
        &self,
    ) -> Result<Vec<ProjectionChange>, PersistenceError> {
        let release = aura_tool_registry::agent_release::release()
            .map_err(|error| PersistenceError::InvalidRequest(error.to_string()))?;
        let entries = release.manifest().entries.clone();
        let runtime = self.runtime_instance_id();
        self.dispatch(move |db| {
            let tx = db.transaction()?;
            let mut missing = BTreeSet::new();
            for entry in entries {
                if matches!(entry.handler, aura_tool_registry::agent_release::AgentBuiltinHandler::Registry(_)) { continue; }
                let instance = crate::tool_entities::load_optional_record::<ToolInstanceRecordV1, _>(&tx,
                    "SELECT record_json,record_hash_json FROM tool_instances WHERE tool_instance_id=?1",
                    [entry.instance.tool_instance_id.to_string()])?;
                let Some(instance) = instance else { continue; };
                if !carry::same_identity(&instance, &entry.instance) { return Err(PersistenceError::ToolEntityIntegrity); }
                let key = ProjectionKey { runtime_instance_id: runtime, projection_kind: ProjectionKind::ToolDefinition,
                    projection_scope: ProjectionScope::ToolDefinition { tool_id: entry.definition.tool_id } };
                let exists = tx.query_row("SELECT EXISTS(SELECT 1 FROM projection_outbox WHERE projection_kind='tool_definition' AND projection_key=?1)",
                    [serde_json::to_string(&key)?], |row| row.get::<_, bool>(0))?;
                if !exists { missing.insert(entry.definition.tool_id); }
            }
            if missing.is_empty() { return Ok(vec![]); }
            let drafts = tool_definition_projection_drafts(&tx, Some(&missing))?;
            let (changes, _) = insert_tool_projections(&tx, runtime, drafts)?;
            tx.commit()?;
            Ok(changes)
        })
    }
}
