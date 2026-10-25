use super::*;

/// Performs the complete schema/registry/input portion of Batch admission. It
/// never starts a Runtime, creates an ExecutionAttempt, acquires a lock, or
/// calls P04. Resource and control preflight stages consume the admitted set
/// before any dispatch is permitted.
pub fn admit_tool_batch(
    batch: ToolBatchRequestV1,
    registry: &ToolRegistrySnapshotV1,
    definitions: Vec<ToolDefinitionRecordV1>,
    authority: &dyn BatchAdmissionAuthorityV1,
) -> Result<ToolBatchAdmissionOutcomeV1, ToolBatchAdmissionInputError> {
    if batch.has_duplicate_tool_call_id() {
        return Ok(ToolBatchAdmissionOutcomeV1::IdentityIntegrityFault {
            code: ToolBatchErrorCode::ToolCallIdentityConflict,
        });
    }
    if batch.validate_hash().is_err() {
        return Ok(reject_batch(&batch, ToolBatchErrorCode::ContentHashMismatch));
    }
    if registry.validate().is_err() {
        return Ok(reject_batch(&batch, ToolBatchErrorCode::RegistrySnapshotMismatch));
    }
    if registry.snapshot_ref != batch.registry_snapshot_ref
        || batch.calls.iter().any(|call| {
            call.definition_ref.as_ref().is_some_and(|reference| {
                reference.registry_snapshot_ref != batch.registry_snapshot_ref
            })
        })
    {
        return Ok(reject_batch(&batch, ToolBatchErrorCode::RegistrySnapshotMismatch));
    }
    if !registry_scope_matches_invocation(registry, &batch.invocation_identity) {
        return Ok(reject_batch(&batch, ToolBatchErrorCode::IdentityMismatch));
    }
    if let Err(issue) = authority.verify_batch_admission(&batch) {
        return Ok(reject_batch(&batch, issue.error_code()));
    }

    let definitions = index_definitions(definitions)?;
    if let Some(code) = validate_control_batch_shape(&batch, registry) {
        let mut rejected = reject_batch(&batch, code);
        if let ToolBatchAdmissionOutcomeV1::BatchRejected { calls, .. } = &mut rejected {
            for (call, result) in batch.calls.iter().zip(calls) {
                if call.definition_ref.is_none() {
                    *result = rejection(
                        call,
                        ToolNotExecutedReasonV1::ToolNotFound,
                        Some(ToolRegistryErrorCode::ToolNotFound.error_code()),
                        None,
                    );
                }
            }
        }
        return Ok(rejected);
    }

    let catalog = registry
        .catalog_entries
        .iter()
        .map(|entry| (entry.tool_id, entry))
        .collect::<BTreeMap<_, _>>();
    // A discoverable tool the model reached through `tools.call` is bound by the
    // snapshot's entry bindings; validation keeps them disjoint from the tools sent.
    let bindings = registry
        .effective_bindings
        .iter()
        .chain(&registry.entry_bindings)
        .map(|binding| (binding.tool_id, binding))
        .collect::<BTreeMap<_, _>>();
    let mut validated_calls = Vec::new();
    let mut not_executed = Vec::new();

    for call in &batch.calls {
        let reference = call.definition_ref.as_ref();
        let Some(catalog_entry) = reference.and_then(|r| catalog.get(&r.tool_id)) else {
            not_executed.push(rejection(
                call,
                ToolNotExecutedReasonV1::ToolNotFound,
                Some(ToolRegistryErrorCode::ToolNotFound.error_code()),
                None,
            ));
            continue;
        };
        let reference = reference.expect("catalog lookup requires a resolved definition");
        if catalog_entry.tool_version != reference.tool_version {
            not_executed.push(rejection(
                call,
                ToolNotExecutedReasonV1::VersionMismatch,
                Some(ToolRegistryErrorCode::VersionMismatch.error_code()),
                None,
            ));
            continue;
        }
        let Some(binding) = bindings.get(&reference.tool_id) else {
            not_executed.push(rejection(call, ToolNotExecutedReasonV1::NotLoaded, None, None));
            continue;
        };
        if binding.tool_version != reference.tool_version
            || binding.tool_instance_id != catalog_entry.tool_instance_id
            || binding.definition_fingerprint != catalog_entry.definition_fingerprint
            || binding.source_contract_fingerprint != catalog_entry.source_contract_fingerprint
        {
            return Ok(reject_batch(&batch, ToolBatchErrorCode::RegistrySnapshotMismatch));
        }
        let definition_key = definition_key(reference)?;
        let Some(definition) = definitions.get(&definition_key) else {
            return Ok(reject_batch(&batch, ToolBatchErrorCode::RegistrySnapshotMismatch));
        };
        if definition.definition_fingerprint != binding.definition_fingerprint
            || definition.tool_id != reference.tool_id
            || definition.tool_version != reference.tool_version
        {
            return Ok(reject_batch(&batch, ToolBatchErrorCode::RegistrySnapshotMismatch));
        }

        match validate_call_input(call, definition) {
            // A well-formed `tools.call` still bound to the entry named no tool this step
            // can run, so nothing is executed for it.
            Ok(()) if aura_tool_contract::is_entry_call(registry, call) => {
                not_executed.push(rejection(
                    call,
                    ToolNotExecutedReasonV1::ToolNotFound,
                    Some(ToolRegistryErrorCode::ToolNotFound.error_code()),
                    None,
                ));
            }
            Ok(()) => validated_calls.push(ValidatedToolCallV1 {
                call: call.clone(),
                definition: definition.clone(),
                binding: (*binding).clone(),
            }),
            Err(rejected) => not_executed.push(rejected),
        }
    }

    validated_calls.sort_by_key(|call| call.call.ordinal);
    not_executed.sort_by_key(|call| call.ordinal);
    Ok(ToolBatchAdmissionOutcomeV1::Admitted(Box::new(AdmittedToolBatchV1 {
        batch,
        validated_calls,
        not_executed,
    })))
}
