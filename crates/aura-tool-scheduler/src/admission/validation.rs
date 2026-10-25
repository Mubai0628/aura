use super::*;

pub(super) fn registry_scope_matches_invocation(
    registry: &ToolRegistrySnapshotV1,
    invocation: &aura_tool_contract::CoreInvocationIdentityV1,
) -> bool {
    match &registry.scope {
        aura_tool_contract::ToolRegistryScopeV1::Workspace { .. } => false,
        aura_tool_contract::ToolRegistryScopeV1::Session {
            session_id,
            turn_task_id,
            agent_run_id,
            ..
        } => {
            *session_id == invocation.session_id
                && *turn_task_id == invocation.turn_task_id
                && *agent_run_id == invocation.agent_run_id
        }
        aura_tool_contract::ToolRegistryScopeV1::ChildAgent {
            session_id,
            turn_task_id,
            child_agent_run_id,
            ..
        } => {
            *session_id == invocation.session_id
                && *turn_task_id == invocation.turn_task_id
                && *child_agent_run_id == invocation.agent_run_id
        }
    }
}

pub(super) fn index_definitions(
    definitions: Vec<ToolDefinitionRecordV1>,
) -> Result<
    BTreeMap<(ToolCallDefinitionId, String), ToolDefinitionRecordV1>,
    ToolBatchAdmissionInputError,
> {
    let mut indexed = BTreeMap::new();
    for definition in definitions {
        definition.validate().map_err(|_| ToolBatchAdmissionInputError::InvalidDefinition)?;
        let key =
            (ToolCallDefinitionId(definition.tool_id), encode_version(&definition.tool_version)?);
        if indexed.insert(key, definition).is_some() {
            return Err(ToolBatchAdmissionInputError::DuplicateDefinition);
        }
    }
    Ok(indexed)
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct ToolCallDefinitionId(aura_domain_common::ToolId);

pub(super) fn definition_key(
    definition_ref: &ToolDefinitionSnapshotRef,
) -> Result<(ToolCallDefinitionId, String), ToolBatchAdmissionInputError> {
    Ok((
        ToolCallDefinitionId(definition_ref.tool_id),
        encode_version(&definition_ref.tool_version)?,
    ))
}

pub(super) fn encode_version(
    version: &aura_tool_contract::ToolVersion,
) -> Result<String, ToolBatchAdmissionInputError> {
    let bytes =
        canonical_json_jcs(version).map_err(|_| ToolBatchAdmissionInputError::InvalidDefinition)?;
    String::from_utf8(bytes).map_err(|_| ToolBatchAdmissionInputError::InvalidDefinition)
}

pub(super) fn validate_control_batch_shape(
    batch: &ToolBatchRequestV1,
    registry: &ToolRegistrySnapshotV1,
) -> Option<ToolBatchErrorCode> {
    let names = batch
        .calls
        .iter()
        .filter_map(|call| {
            let reference = call.definition_ref.as_ref()?;
            registry
                .catalog_entries
                .iter()
                .find(|entry| {
                    entry.tool_id == reference.tool_id
                        && entry.tool_version == reference.tool_version
                })
                .map(|entry| entry.canonical_name.as_str())
        })
        .collect::<Vec<_>>();
    let completes = names.iter().filter(|name| **name == "normal.complete").count();
    let reviews = names.iter().filter(|name| **name == "task_lineage.review_completion").count();
    if completes > 1 || reviews > 1 {
        return Some(ToolBatchErrorCode::CompletionBatchInvalid);
    }
    if completes > 0 {
        let complete_only = batch.calls.len() == 1 && completes == 1;
        let optional_review = batch.calls.len() == 2 && completes == 1 && reviews == 1;
        if !complete_only && !optional_review {
            return Some(ToolBatchErrorCode::CompletionBatchInvalid);
        }
    }
    if names.contains(&"goal.check") && names.contains(&"goal.complete") {
        return Some(ToolBatchErrorCode::CompletionBatchInvalid);
    }
    if names.contains(&"mode.disposition") && batch.calls.len() != 1 {
        return Some(ToolBatchErrorCode::CompletionBatchInvalid);
    }
    None
}

pub(super) fn validate_call_input(
    call: &aura_tool_contract::NormalizedToolCallV1,
    definition: &ToolDefinitionRecordV1,
) -> Result<(), ToolPreflightRejectionV1> {
    let (arguments, arguments_hash) = match &call.input {
        NormalizedToolCallInputStateV1::Ready { arguments, arguments_hash, .. } => {
            (arguments, arguments_hash)
        }
        NormalizedToolCallInputStateV1::InvalidInput { issue, detail } => {
            return Err(input_rejection(call, *issue, detail.clone().map(Box::new)));
        }
    };
    let canonical_arguments = canonical_json_jcs(arguments).map_err(|_| {
        rejection(
            call,
            ToolNotExecutedReasonV1::SchemaValidationFailed,
            Some(ToolSchemaErrorCode::ArgumentsInvalid.error_code()),
            None,
        )
    })?;
    if canonical_arguments.len() > MAX_PROVIDER_ARGUMENTS_BYTES {
        return Err(rejection(
            call,
            ToolNotExecutedReasonV1::SchemaValidationFailed,
            Some(ToolSchemaErrorCode::ArgumentsTooLarge.error_code()),
            None,
        ));
    }
    if canonical_sha256(arguments).ok().as_ref() != Some(arguments_hash) {
        return Err(rejection(
            call,
            ToolNotExecutedReasonV1::SchemaValidationFailed,
            Some(ToolSchemaErrorCode::HashMismatch.error_code()),
            None,
        ));
    }
    let compiled = CompiledToolSchemaV1::compile(&definition.input_schema.clone().into_document())
        .map_err(|_| {
            rejection(
                call,
                ToolNotExecutedReasonV1::SchemaValidationFailed,
                Some(ToolSchemaErrorCode::ValidatorCompilationFailed.error_code()),
                None,
            )
        })?;
    let report = compiled.validate(arguments);
    if !report.valid {
        return Err(rejection(
            call,
            ToolNotExecutedReasonV1::SchemaValidationFailed,
            Some(ToolSchemaErrorCode::ArgumentsInvalid.error_code()),
            Some(report),
        ));
    }
    Ok(())
}

pub fn validate_unique_admitted_results(
    admitted: &AdmittedToolBatchV1,
) -> Result<(), ToolBatchAdmissionInputError> {
    let mut ids = BTreeSet::new();
    if admitted
        .validated_calls
        .iter()
        .map(|call| call.call.tool_call_id)
        .chain(admitted.not_executed.iter().map(|call| call.tool_call_id))
        .any(|tool_call_id| !ids.insert(tool_call_id))
        || ids.len() != admitted.batch.calls.len()
    {
        return Err(ToolBatchAdmissionInputError::InvalidDefinition);
    }
    Ok(())
}
