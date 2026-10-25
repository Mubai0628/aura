use super::*;

fn require_credential_instance(
    tx: &Transaction<'_>,
    id: ToolInstanceId,
    revision: Revision,
) -> Result<(ToolInstanceRecordV1, ToolDefinitionRecordV1, Option<ToolPackageId>), PersistenceError>
{
    let instance = load_optional_record::<ToolInstanceRecordV1, _>(
        tx,
        "SELECT record_json, record_hash_json FROM tool_instances WHERE tool_instance_id=?1",
        [id.to_string()],
    )?
    .ok_or(PersistenceError::ToolEntityNotFound)?;
    if instance.entity_revision != revision {
        return Err(PersistenceError::ToolEntityRevisionConflict);
    }
    let definition = load_optional_record::<ToolDefinitionRecordV1,_>(tx,
        "SELECT record_json, record_hash_json FROM tool_definitions WHERE tool_id=?1 AND tool_version_json=?2",
        params![instance.tool_id.to_string(),encode_json(&instance.tool_version)?])?
        .ok_or(PersistenceError::ToolEntityIntegrity)?;
    if definition.tool_package_id.is_some() {
        let (instance, definition, package) = require_mutable_instance(tx, id, revision)?;
        return Ok((instance, definition, Some(package)));
    }
    if !matches!(&definition.runtime,aura_tool_contract::ToolRuntimeDescriptorV1::ExternalSource {source_contract_key}
        if source_contract_key == "aura.builtin.http.request.v1")
    {
        return Err(PersistenceError::ToolEntityStateConflict);
    }
    // Builtin release instances are immutable. The separate binding revision is the
    // authority changed here and is frozen into every credential-bearing operation.
    Ok((instance, definition, None))
}

#[allow(clippy::too_many_arguments)]
fn finish_credential_mutation(
    tx: Transaction<'_>,
    runtime: aura_domain_common::RuntimeInstanceId,
    target: (Option<ToolPackageId>, ToolId),
    revision: Revision,
    now: UtcTimestamp,
    event_ids: Vec<EventId>,
    method: &'static str,
    correlation: CorrelationId,
    result: ToolMutationResultV1,
) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
    if let Some(package) = target.0 {
        return finish_package_mutation(
            tx,
            runtime,
            package,
            revision,
            now,
            event_ids,
            method,
            correlation,
            result,
        );
    }
    let registry_revision = advance_registry_revision(&tx, now)?;
    let drafts = crate::tool_builtin_release::tool_definition_projection_drafts(
        &tx,
        Some(&[target.1].into_iter().collect()),
    )?;
    let (projection_changes, projection_outbox_ids) =
        insert_tool_projections(&tx, runtime, drafts)?;
    complete_tool_mutation_command_success_in_transaction(&tx, method, correlation, result, now)?;
    tx.commit()?;
    Ok(ToolSettingsMutationCommitV1 {
        changed: true,
        target_entity_revision: revision,
        registry_revision,
        event_ids,
        projection_changes,
        projection_outbox_ids,
    })
}

impl PersistenceCoordinator {
    pub fn commit_tool_instance_credential_settings_change(
        &self,
        request: ToolInstanceCredentialSettingsChangeRequestV1,
    ) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (mut instance, _, package_id) = require_credential_instance(
                &transaction,
                request.tool_instance_id,
                request.expected_entity_revision,
            )?;
            Self::authorize_tool_credential_binding_in_transaction(
                &transaction,
                &ToolCredentialBindingAuthorizationV1 {
                    authenticated_owner: request.authenticated_owner,
                    tool_instance_id: request.tool_instance_id,
                    credential_slot_key: request.credential_slot_key.clone(),
                    placement: request.placement.clone(),
                    credential_id: request.credential_id,
                    credential_revision: request.credential_revision,
                    workspace_id: request.workspace_id,
                    session_id: request.session_id,
                    expected_candidate_set_revision: request.expected_candidate_set_revision,
                },
            )?;
            let current = load_optional_record::<ToolInstanceCredentialBindingRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_instance_credential_bindings
                 WHERE tool_instance_id=?1",
                [request.tool_instance_id.to_string()],
            )?;
            if current.as_ref().map(|record| record.binding_revision)
                != request.expected_binding_revision
            {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            if current.as_ref().is_some_and(|record| {
                record.credential_id == request.credential_id
                    && record.credential_revision == request.credential_revision
                    && record.credential_slot_key == request.credential_slot_key
                    && record.placement == request.placement
            }) {
                return unchanged_commit(
                    transaction,
                    instance.entity_revision,
                    TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1,
                    request.correlation_id,
                    request.changed_at,
                    ToolMutationResultV1::InstanceCommitted {
                        tool_instance_id: request.tool_instance_id,
                        entity_revision: instance.entity_revision,
                    },
                );
            }
            let binding_revision =
                next_entity_revision(current.as_ref().map(|record| record.binding_revision))?;
            let record = ToolInstanceCredentialBindingRecordV1::new(
                request.tool_instance_id,
                request.credential_slot_key.clone(),
                request.credential_id,
                request.credential_revision,
                request.placement.clone(),
                binding_revision,
                request.changed_at,
            )
            .map_err(|_| PersistenceError::ToolEntityIntegrity)?;
            let (record_json, record_hash_json) = encode_record(&record)?;
            transaction.execute(
                "INSERT INTO tool_instance_credential_bindings(
                    tool_instance_id, credential_slot_key, credential_id, credential_revision,
                    placement_json, binding_revision, binding_hash_json, record_json,
                    record_hash_json, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(tool_instance_id) DO UPDATE SET
                    credential_slot_key=excluded.credential_slot_key,
                    credential_id=excluded.credential_id,
                    credential_revision=excluded.credential_revision,
                    placement_json=excluded.placement_json,
                    binding_revision=excluded.binding_revision,
                    binding_hash_json=excluded.binding_hash_json,
                    record_json=excluded.record_json, record_hash_json=excluded.record_hash_json,
                    updated_at=excluded.updated_at",
                params![
                    request.tool_instance_id.to_string(),
                    request.credential_slot_key.as_str(),
                    request.credential_id.to_string(),
                    i64::try_from(request.credential_revision.value())
                        .map_err(|_| PersistenceError::ToolEntityIntegrity)?,
                    encode_json(&request.placement)?,
                    sqlite_revision(binding_revision)?,
                    encode_json(&record.binding_hash)?,
                    record_json,
                    record_hash_json,
                    request.changed_at.to_string(),
                ],
            )?;
            if package_id.is_some() {
                bump_instance(&transaction, &mut instance, request.changed_at)?;
            }
            let event_id = EventId::new();
            insert_event(
                &transaction,
                &credential_event(
                    event_id,
                    &instance,
                    Some(request.credential_id),
                    Some(request.credential_revision),
                    Some(binding_revision),
                    request.correlation_id,
                    request.changed_at,
                )?,
            )?;
            crate::tool_builtin_release::clear_rebind_reason(
                &transaction,
                request.tool_instance_id,
            )?;
            finish_credential_mutation(
                transaction,
                runtime_instance_id,
                (package_id, instance.tool_id),
                instance.entity_revision,
                request.changed_at,
                vec![event_id],
                TOOL_INSTANCE_BIND_CREDENTIAL_METHOD_V1,
                request.correlation_id,
                ToolMutationResultV1::InstanceCommitted {
                    tool_instance_id: instance.tool_instance_id,
                    entity_revision: instance.entity_revision,
                },
            )
        })
    }

    pub fn commit_tool_instance_credential_clear(
        &self,
        request: ToolInstanceCredentialClearRequestV1,
    ) -> Result<ToolSettingsMutationCommitV1, PersistenceError> {
        let runtime_instance_id = self.runtime_instance_id();
        self.dispatch(move |connection| {
            let transaction = connection.transaction()?;
            let (mut instance, _, package_id) = require_credential_instance(
                &transaction,
                request.tool_instance_id,
                request.expected_entity_revision,
            )?;
            let current = load_optional_record::<ToolInstanceCredentialBindingRecordV1, _>(
                &transaction,
                "SELECT record_json, record_hash_json FROM tool_instance_credential_bindings
                 WHERE tool_instance_id=?1",
                [request.tool_instance_id.to_string()],
            )?
            .ok_or(PersistenceError::ToolEntityNotFound)?;
            if current.binding_revision != request.expected_binding_revision {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            let removed = transaction.execute(
                "DELETE FROM tool_instance_credential_bindings
                 WHERE tool_instance_id=?1 AND binding_revision=?2",
                params![
                    request.tool_instance_id.to_string(),
                    sqlite_revision(request.expected_binding_revision)?,
                ],
            )?;
            if removed != 1 {
                return Err(PersistenceError::ToolEntityRevisionConflict);
            }
            if package_id.is_some() {
                bump_instance(&transaction, &mut instance, request.changed_at)?;
            }
            let event_id = EventId::new();
            insert_event(
                &transaction,
                &credential_event(
                    event_id,
                    &instance,
                    None,
                    None,
                    None,
                    request.correlation_id,
                    request.changed_at,
                )?,
            )?;
            finish_credential_mutation(
                transaction,
                runtime_instance_id,
                (package_id, instance.tool_id),
                instance.entity_revision,
                request.changed_at,
                vec![event_id],
                TOOL_INSTANCE_CLEAR_CREDENTIAL_METHOD_V1,
                request.correlation_id,
                ToolMutationResultV1::InstanceCommitted {
                    tool_instance_id: instance.tool_instance_id,
                    entity_revision: instance.entity_revision,
                },
            )
        })
    }
}
