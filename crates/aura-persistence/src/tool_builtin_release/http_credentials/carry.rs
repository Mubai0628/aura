use super::*;
use crate::tool_entities::{load_all_records, load_optional_record};
use aura_tool_registry::agent_release::AgentBuiltinEntry;

pub(super) fn same_identity(a: &ToolInstanceRecordV1, b: &ToolInstanceRecordV1) -> bool {
    a.tool_instance_id == b.tool_instance_id
        && a.tool_id == b.tool_id
        && a.tool_version == b.tool_version
        && a.source_kind == b.source_kind
        && a.source_contract_fingerprint == b.source_contract_fingerprint
        && a.entity_revision >= b.entity_revision
        && a.overrides.validate().is_ok()
}
pub(super) fn previous(
    tx: &Transaction<'_>,
    entry: &AgentBuiltinEntry,
) -> Result<Option<ToolInstanceRecordV1>, PersistenceError> {
    let mut rows = load_all_records::<ToolInstanceRecordV1>(
        tx,
        "SELECT record_json,record_hash_json FROM tool_instances ORDER BY tool_instance_id",
    )?
    .into_iter()
    .filter(|i| i.tool_id == entry.definition.tool_id && i.source_kind == ToolSourceKindV1::BuiltIn)
    .collect::<Vec<_>>();
    for row in &rows {
        row.overrides.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    }
    rows.sort_by_key(|i| i.tool_version.definition_revision);
    let Some(last) = rows.pop() else { return Ok(None) };
    if last.tool_version.definition_revision > entry.definition.tool_version.definition_revision
        || (last.tool_version == entry.definition.tool_version
            && last.tool_instance_id != entry.instance.tool_instance_id)
        || rows.last().is_some_and(|i| i.tool_version == last.tool_version)
    {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    Ok(Some(last))
}
pub(super) fn configuration(
    tx: &Transaction<'_>,
    old: &ToolInstanceRecordV1,
    new: &AgentBuiltinEntry,
    now: UtcTimestamp,
) -> Result<(), PersistenceError> {
    if let Some(old_config) = load_optional_record::<ToolInstanceConfigurationRecordV1, _>(
        tx,
        "SELECT record_json,record_hash_json FROM tool_instance_configurations WHERE tool_instance_id=?1",
        [old.tool_instance_id.to_string()],
    )? {
        old_config.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let config = ToolInstanceConfigurationRecordV1::new(
            new.instance.tool_instance_id,
            old_config.configuration_revision,
            old_config.instance_config_schema_hash,
            old_config.configuration,
            old_config.secret_scan_evidence_hash,
            now,
        )
        .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
        let (value, hash) = encode_record(&config)?;
        tx.execute(
            "INSERT INTO tool_instance_configurations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                config.tool_instance_id.to_string(),
                sqlite_revision(config.configuration_revision)?,
                encode_json(&config.instance_config_schema_hash)?,
                encode_json(&config.configuration)?,
                encode_json(&config.configuration_hash)?,
                encode_json(&config.secret_scan_evidence_hash)?,
                value,
                hash,
                now.to_string()
            ],
        )?;
    }
    let definition = load_optional_record::<ToolDefinitionRecordV1,_>(tx,"SELECT record_json,record_hash_json FROM tool_definitions WHERE tool_id=?1 AND tool_version_json=?2",params![old.tool_id.to_string(),encode_json(&old.tool_version)?])?.ok_or(PersistenceError::ToolEntityIntegrity)?;
    definition.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let old_slot = crate::tool_credential::exact_runtime_slot(&definition.runtime);
    let new_slot = crate::tool_credential::exact_runtime_slot(&new.definition.runtime);
    if old_slot != new_slot {
        tx.execute(
            "INSERT INTO builtin_credential_rebind_required VALUES(?1,?2)",
            params![new.instance.tool_instance_id.to_string(), "credential_slot_changed"],
        )?;
        return Ok(());
    }
    let Some(binding) = load_optional_record::<ToolInstanceCredentialBindingRecordV1, _>(
        tx,
        "SELECT record_json,record_hash_json FROM tool_instance_credential_bindings WHERE tool_instance_id=?1",
        [old.tool_instance_id.to_string()],
    )?
    else {
        return Ok(());
    };
    binding.validate().map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    if new_slot.as_ref().is_none_or(|(slot, placement)| {
        *slot != binding.credential_slot_key || *placement != binding.placement
    }) {
        return Err(PersistenceError::ToolEntityIntegrity);
    }
    // Keep the original CredentialId/revision. Its owner, scope, revocation and
    // allowed origins stay in the existing authority row; no secret is copied.
    let binding = ToolInstanceCredentialBindingRecordV1::new(
        new.instance.tool_instance_id,
        binding.credential_slot_key,
        binding.credential_id,
        binding.credential_revision,
        binding.placement,
        binding.binding_revision,
        now,
    )
    .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
    let (value, hash) = encode_record(&binding)?;
    tx.execute("INSERT INTO tool_instance_credential_bindings(tool_instance_id,credential_slot_key,credential_id,credential_revision,placement_json,binding_revision,binding_hash_json,record_json,record_hash_json,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",params![binding.tool_instance_id.to_string(),binding.credential_slot_key.as_str(),binding.credential_id.to_string(),i64::try_from(binding.credential_revision.value()).map_err(|_|PersistenceError::ToolEntityIntegrity)?,encode_json(&binding.placement)?,sqlite_revision(binding.binding_revision)?,encode_json(&binding.binding_hash)?,value,hash,now.to_string()])?;
    Ok(())
}
pub(crate) fn rebind_reason(
    db: &rusqlite::Connection,
    id: ToolInstanceId,
) -> Result<Option<String>, PersistenceError> {
    Ok(db
        .query_row(
            "SELECT reason FROM builtin_credential_rebind_required WHERE tool_instance_id=?1",
            [id.to_string()],
            |r| r.get(0),
        )
        .optional()?)
}
pub(crate) fn clear_rebind_reason(
    db: &rusqlite::Connection,
    id: ToolInstanceId,
) -> Result<(), PersistenceError> {
    db.execute(
        "DELETE FROM builtin_credential_rebind_required WHERE tool_instance_id=?1",
        [id.to_string()],
    )?;
    Ok(())
}
