mod credential;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    CorrelationId, CredentialId, CredentialRevision, EventId, PrincipalId, Revision, SessionId,
    UtcTimestamp, WorkspaceId,
};
use aura_domain_common::{ToolId, ToolInstanceId, ToolPackageId};
use aura_projection_contract::ProjectionChange;
use aura_tool_contract::{
    CompiledToolSchemaV1, ToolEnabledChangedPayloadV1, ToolInstanceConfigurationChangedPayloadV1,
    ToolInstanceCredentialBindingChangedPayloadV1, ToolLifecycleEventV1,
};
use aura_tool_domain::{
    TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1, TOOL_INSTANCE_CLEAR_CREDENTIAL_METHOD_V1,
    TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1, TOOL_SET_ENABLED_METHOD_V1,
    ToolCredentialPlacementV1, ToolDefinitionRecordV1, ToolInstanceConfigurationRecordV1,
    ToolInstanceCredentialBindingRecordV1, ToolInstanceRecordV1, ToolMutationResultV1,
    ToolPackageLifecycleV1, ToolPackageRecordV1,
};
use rusqlite::{Transaction, params};

use crate::{
    EventActor, PersistenceCoordinator, PersistenceError, ToolCredentialBindingAuthorizationV1,
    coordinator::insert_event,
    tool::encode_canonical,
    tool_command::complete_tool_mutation_command_success_in_transaction,
    tool_entities::{
        advance_registry_revision, encode_json, encode_record, load_bound_records,
        load_optional_record, next_entity_revision, read_registry_revision, sqlite_revision,
        tool_event,
    },
    tool_package_reads::package_projection_drafts,
    tool_projection::insert_tool_projections,
};

#[derive(Clone, Debug)]
pub struct ToolDefinitionEnabledChangeRequestV1 {
    pub tool_id: ToolId,
    pub expected_entity_revision: Revision,
    pub enabled: bool,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolInstanceConfigurationSettingsChangeRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub expected_entity_revision: Revision,
    pub expected_configuration_revision: Option<Revision>,
    pub expected_schema_hash: CanonicalHashV1,
    pub configuration: serde_json::Value,
    pub secret_scan_evidence_hash: CanonicalHashV1,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolInstanceCredentialSettingsChangeRequestV1 {
    pub authenticated_owner: PrincipalId,
    pub tool_instance_id: ToolInstanceId,
    pub credential_slot_key: aura_tool_contract::CredentialSlotKey,
    pub placement: ToolCredentialPlacementV1,
    pub expected_entity_revision: Revision,
    pub expected_binding_revision: Option<Revision>,
    pub expected_candidate_set_revision: Revision,
    pub credential_id: CredentialId,
    pub credential_revision: CredentialRevision,
    pub workspace_id: Option<WorkspaceId>,
    pub session_id: Option<SessionId>,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug)]
pub struct ToolInstanceCredentialClearRequestV1 {
    pub tool_instance_id: ToolInstanceId,
    pub expected_entity_revision: Revision,
    pub expected_binding_revision: Revision,
    pub changed_at: UtcTimestamp,
    pub correlation_id: CorrelationId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToolSettingsMutationCommitV1 {
    pub changed: bool,
    pub target_entity_revision: Revision,
    pub registry_revision: Revision,
    pub event_ids: Vec<EventId>,
    pub projection_changes: Vec<ProjectionChange>,
    pub projection_outbox_ids: Vec<i64>,
}

impl PersistenceCoordinator {
    pub fn commit_tool_definition_enabled_change(
        &self,
        request: ToolDefinitionEnabledChangeRequestV1,
    ) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let definitions = load_bound_records::<ToolDefinitionRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_definitions
                 WHERE tool_id=?1 AND retired_at IS NULL ORDER BY definition_revision",
                [request.tool_id.to_string()],
            )?;
            let package_id = definitions
                .first()
                .and_then(|definition| definition.tool_package_id)
                .ok_or(PersistenceError::ToolEntityStateConflict)?;
            if definitions.iter().any(|definition| definition.tool_package_id != Some(package_id)) {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let package = load_optional_record::<ToolPackageRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_packages WHERE tool_package_id=?1",
                [package_id.to_string()],
            )?
            .ok_or(PersistenceError::ToolEntityIntegrity)?;
            if package.lifecycle != ToolPackageLifecycleV1::Installed || package.deletion_pending {
                return Err(PersistenceError::ToolEntityStateConflict);
            }
            let matching = crate::tool_package_reads::membership::active_definitions(
                &transaction,
                &package,
                package_definitions(&transaction, package_id)?,
            )?
            .into_iter()
            .filter(|definition| definition.tool_id == request.tool_id)
            .collect::<Vec<_>>();
            let [definition] = matching.as_slice() else {
                return Err(PersistenceError::ToolEntityIntegrity);
            };
            let mut definition = definition.clone();
            if definition.entity_revision != request.expected_entity_revision {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            let mut instances = load_bound_records::<ToolInstanceRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_instances
                 WHERE tool_id=?1 ORDER BY tool_instance_id",
                [request.tool_id.to_string()],
            )?;
            if instances.is_empty() {
                return Err(PersistenceError::ToolEntityIntegrity);
            }
            let mut event_ids = Vec::new();
            for instance in &mut instances {
                if instance.enabled == request.enabled {
                    continue;
                }
                update_instance_enabled(
                    &transaction,
                    instance,
                    request.enabled,
                    request.changed_at,
                )?;
                let event_id = EventId::new();
                insert_event(
                    &transaction,
                    &enabled_event(event_id, instance, request.correlation_id, request.changed_at)?,
                )?;
                event_ids.push(event_id);
            }
            if event_ids.is_empty() {
                return unchanged_commit(
                    transaction,
                    definition.entity_revision,
                    TOOL_SET_ENABLED_METHOD_V1,
                    request.correlation_id,
                    request.changed_at,
                    ToolMutationResultV1::ToolCommitted {
                        tool_id: request.tool_id,
                        entity_revision: definition.entity_revision,
                    },
                );
            }
            definition.entity_revision = next_entity_revision(Some(definition.entity_revision))?;
            let (record_json, record_hash_json) = encode_record(&definition)?;
            let changed = transaction.execute(
                "UPDATE tool_definitions SET entity_revision=?1, record_json=?2,
                    record_hash_json=?3 WHERE tool_id=?4 AND tool_version_json=?5
                    AND entity_revision=?6",
                params![
                    sqlite_revision(definition.entity_revision)?,
                    record_json,
                    record_hash_json,
                    definition.tool_id.to_string(),
                    encode_json(&definition.tool_version)?,
                    sqlite_revision(request.expected_entity_revision)?,
                ],
            )?;
            if changed != 1 {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            finish_package_mutation(
                transaction,
                runtime_instance_id,
                package_id,
                definition.entity_revision,
                request.changed_at,
                event_ids,
                TOOL_SET_ENABLED_METHOD_V1,
                request.correlation_id,
                ToolMutationResultV1::ToolCommitted {
                    tool_id: definition.tool_id,
                    entity_revision: definition.entity_revision,
                },
            )
        })
    }

    pub fn commit_tool_instance_configuration_settings_change(
        &self,
        request: ToolInstanceConfigurationSettingsChangeRequestV1,
    ) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
        let schema = self
            .instance_configuration_schema(request.tool_instance_id)?
            .ok_or(PersistenceError::ToolEntityStateConflict)?;
        if schema.schema_hash() != &request.expected_schema_hash
            || !CompiledToolSchemaV1::compile(schema.canonical_document())
                .map_err(|_| PersistenceError::ToolEntityIntegrity)?
                .validate(&request.configuration)
                .valid
        {
            return Err(PersistenceError::ToolEntityStateConflict);
        }
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (mut instance, definition, package_id) = require_mutable_instance(
                &transaction,
                request.tool_instance_id,
                request.expected_entity_revision,
            )?;
            let current = load_optional_record::<ToolInstanceConfigurationRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_instance_configurations
                 WHERE tool_instance_id=?1",
                [request.tool_instance_id.to_string()],
            )?;
            if current.as_ref().map(|record| record.configuration_revision)
                != request.expected_configuration_revision
            {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            if current.as_ref().is_some_and(|record| {
                record.configuration == request.configuration
                    && record.instance_config_schema_hash == request.expected_schema_hash
            }) {
                return unchanged_commit(
                    transaction,
                    instance.entity_revision,
                    TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1,
                    request.correlation_id,
                    request.changed_at,
                    ToolMutationResultV1::InstanceCommitted {
                        tool_instance_id: request.tool_instance_id,
                        entity_revision: instance.entity_revision,
                    },
                );
            }
            let configuration_revision =
                next_entity_revision(current.as_ref().map(|record| record.configuration_revision))?;
            let record = ToolInstanceConfigurationRecordV1::new(
                request.tool_instance_id,
                configuration_revision,
                request.expected_schema_hash.clone(),
                request.configuration,
                request.secret_scan_evidence_hash,
                request.changed_at,
            )
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let (record_json, record_hash_json) = encode_record(&record)?;
            transaction.execute(
                "INSERT INTO tool_instance_configurations(
                    tool_instance_id, configuration_revision,
                    instance_config_schema_hash_json, configuration_json,
                    configuration_hash_json, secret_scan_evidence_hash_json,
                    record_json, record_hash_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(tool_instance_id) DO UPDATE SET
                    configuration_revision=excluded.configuration_revision,
                    instance_config_schema_hash_json=excluded.instance_config_schema_hash_json,
                    configuration_json=excluded.configuration_json,
                    configuration_hash_json=excluded.configuration_hash_json,
                    secret_scan_evidence_hash_json=excluded.secret_scan_evidence_hash_json,
                    record_json=excluded.record_json, record_hash_json=excluded.record_hash_json,
                    updated_at=excluded.updated_at",
                params![
                    request.tool_instance_id.to_string(),
                    sqlite_revision(configuration_revision)?,
                    encode_json(&record.instance_config_schema_hash)?,
                    encode_canonical(&record.configuration)?,
                    encode_json(&record.configuration_hash)?,
                    encode_json(&record.secret_scan_evidence_hash)?,
                    record_json,
                    record_hash_json,
                    request.changed_at.to_string(),
                ],
            )?;
            bump_instance(&transaction, &mut instance, request.changed_at)?;
            let event_id = EventId::new();
            insert_event(
                &transaction,
                &tool_event(
                    event_id,
                    EventActor::User,
                    request.correlation_id,
                    ToolLifecycleEventV1::ToolInstanceConfigurationChanged(
                        ToolInstanceConfigurationChangedPayloadV1 {
                            tool_instance_id: request.tool_instance_id,
                            configuration_revision,
                            schema_hash: record.instance_config_schema_hash,
                            configuration_hash: record.configuration_hash,
                            entity_revision: instance.entity_revision,
                            changed_at: request.changed_at,
                        },
                    ),
                    request.changed_at,
                )?,
            )?;
            let _ = definition;
            finish_package_mutation(
                transaction,
                runtime_instance_id,
                package_id,
                instance.entity_revision,
                request.changed_at,
                vec![event_id],
                TOOL_INSTANCE_UPDATE_CONFIGURATION_METHOD_V1,
                request.correlation_id,
                ToolMutationResultV1::InstanceCommitted {
                    tool_instance_id: instance.tool_instance_id,
                    entity_revision: instance.entity_revision,
                },
            )
        })
    }
}

fn require_mutable_instance(
    transaction: &Transaction<'_>,
    tool_instance_id: ToolInstanceId,
    expected_entity_revision: Revision,
) -> Result<(ToolInstanceRecordV1, ToolDefinitionRecordV1, ToolPackageId), PersistenceError> {
    let instance = load_optional_record::<ToolInstanceRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_instances WHERE tool_instance_id=?1",
        [tool_instance_id.to_string()],
    )?
    .ok_or(PersistenceError::ToolEntityNotFound)?;
    if instance.entity_revision != expected_entity_revision {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    let definition = load_optional_record::<ToolDefinitionRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_id=?1 AND tool_version_json=?2",
        params![instance.tool_id.to_string(), encode_json(&instance.tool_version)?],
    )?
    .ok_or(PersistenceError::ToolEntityIntegrity)?;
    let package_id = definition.tool_package_id.ok_or(PersistenceError::ToolEntityStateConflict)?;
    let package = load_optional_record::<ToolPackageRecordV1, _>(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_packages WHERE tool_package_id=?1",
        [package_id.to_string()],
    )?
    .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if package.lifecycle != ToolPackageLifecycleV1::Installed || package.deletion_pending {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    Ok((instance, definition, package_id))
}

fn package_definitions(
    transaction: &Transaction<'_>,
    tool_package_id: ToolPackageId,
) -> Result<Vec<ToolDefinitionRecordV1>, PersistenceError> {
    load_bound_records(
        transaction,
        "SELECT record_json, record_hash_json FROM tool_definitions
         WHERE tool_package_id=?1 AND retired_at IS NULL ORDER BY tool_id, definition_revision",
        [tool_package_id.to_string()],
    )
}

fn bump_instance(
    transaction: &Transaction<'_>,
    instance: &mut ToolInstanceRecordV1,
    changed_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    let previous = instance.entity_revision;
    instance.entity_revision = next_entity_revision(Some(previous))?;
    instance.updated_at = changed_at;
    let (record_json, record_hash_json) = encode_record(instance)?;
    let changed = transaction.execute(
        "UPDATE tool_instances SET entity_revision=?1, record_json=?2,
            record_hash_json=?3, updated_at=?4
         WHERE tool_instance_id=?5 AND entity_revision=?6",
        params![
            sqlite_revision(instance.entity_revision)?,
            record_json,
            record_hash_json,
            changed_at.to_string(),
            instance.tool_instance_id.to_string(),
            sqlite_revision(previous)?,
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    Ok(())
}

fn update_instance_enabled(
    transaction: &Transaction<'_>,
    instance: &mut ToolInstanceRecordV1,
    enabled: bool,
    changed_at: UtcTimestamp,
) -> Result<(), PersistenceError> {
    instance.enabled = enabled;
    let previous = instance.entity_revision;
    instance.entity_revision = next_entity_revision(Some(previous))?;
    instance.updated_at = changed_at;
    let (record_json, record_hash_json) = encode_record(instance)?;
    let changed = transaction.execute(
        "UPDATE tool_instances SET enabled=?1, entity_revision=?2, record_json=?3,
            record_hash_json=?4, updated_at=?5
         WHERE tool_instance_id=?6 AND entity_revision=?7",
        params![
            enabled,
            sqlite_revision(instance.entity_revision)?,
            record_json,
            record_hash_json,
            changed_at.to_string(),
            instance.tool_instance_id.to_string(),
            sqlite_revision(previous)?,
        ],
    )?;
    if changed != 1 {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    Ok(())
}

fn enabled_event(
    event_id: EventId,
    instance: &ToolInstanceRecordV1,
    correlation_id: CorrelationId,
    changed_at: UtcTimestamp,
) -> Result<crate::PersistentEventDraft, PersistenceError> {
    tool_event(
        event_id,
        EventActor::User,
        correlation_id,
        ToolLifecycleEventV1::ToolEnabledChanged(ToolEnabledChangedPayloadV1 {
            tool_instance_id: instance.tool_instance_id,
            enabled: instance.enabled,
            entity_revision: instance.entity_revision,
            changed_at,
        }),
        changed_at,
    )
}

#[allow(clippy::too_many_arguments)]
fn credential_event(
    event_id: EventId,
    instance: &ToolInstanceRecordV1,
    credential_id: Option<CredentialId>,
    credential_revision: Option<CredentialRevision>,
    binding_revision: Option<Revision>,
    correlation_id: CorrelationId,
    changed_at: UtcTimestamp,
) -> Result<crate::PersistentEventDraft, PersistenceError> {
    tool_event(
        event_id,
        EventActor::User,
        correlation_id,
        ToolLifecycleEventV1::ToolInstanceCredentialBindingChanged(
            ToolInstanceCredentialBindingChangedPayloadV1 {
                tool_instance_id: instance.tool_instance_id,
                credential_id,
                credential_revision,
                binding_revision,
                entity_revision: instance.entity_revision,
                changed_at,
            },
        ),
        changed_at,
    )
}

fn unchanged_commit(
    transaction: Transaction<'_>,
    target_entity_revision: Revision,
    method_name: &'static str,
    correlation_id: CorrelationId,
    changed_at: UtcTimestamp,
    result: ToolMutationResultV1,
) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
    complete_tool_mutation_command_success_in_transaction(
        &transaction,
        method_name,
        correlation_id,
        result,
        changed_at,
    )?;
    let commit = ToolSettingsMutationCommitV1 {
        changed: false,
        target_entity_revision,
        registry_revision: read_registry_revision(&transaction)?,
        event_ids: Vec::new(),
        projection_changes: Vec::new(),
        projection_outbox_ids: Vec::new(),
    };
    transaction.commit()?;
    Ok(commit)
}

// These values are kept explicit so the commit boundary visibly binds its
// authority, revisions, event set, journal result and projection transaction.
#[allow(clippy::too_many_arguments)]
fn finish_package_mutation(
    transaction: Transaction<'_>,
    runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    tool_package_id: ToolPackageId,
    target_entity_revision: Revision,
    changed_at: UtcTimestamp,
    event_ids: Vec<EventId>,
    method_name: &'static str,
    correlation_id: CorrelationId,
    result: ToolMutationResultV1,
) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
    let registry_revision = advance_registry_revision(&transaction, changed_at)?;
    let drafts = package_projection_drafts(&transaction, tool_package_id)?;
    let (projection_changes, projection_outbox_ids) =
        insert_tool_projections(&transaction, runtime_instance_id, drafts)?;
    complete_tool_mutation_command_success_in_transaction(
        &transaction,
        method_name,
        correlation_id,
        result,
        changed_at,
    )?;
    transaction.commit()?;
    Ok(ToolSettingsMutationCommitV1 {
        changed: true,
        target_entity_revision,
        registry_revision,
        event_ids,
        projection_changes,
        projection_outbox_ids,
    })
}
