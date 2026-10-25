use super::*;

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BuiltinSourceContractFingerprintMaterialV1<'a> {
    tool_id: ToolId,
    tool_version: &'a ToolVersion,
    tool_instance_id: ToolInstanceId,
    tool_instance_revision: Revision,
    definition_fingerprint: &'a CanonicalHashV1,
    execution_binding: ToolInstanceExecutionBindingV1,
}

pub(super) fn build_builtin_release_entries()
-> Result<Vec<SignedBuiltinReleaseEntryV1>, BuiltinToolDefinitionSetError> {
    let definitions = build_builtin_definitions()?;
    // R4: one stable instance id per definition, in the same order. `zip` truncates to the shorter
    // side, so a definition added without a matching id here would silently vanish and then fail the
    // registry cross-check with a confusing error; the length is asserted below so that mismatch is
    // named for what it is.
    let instance_ids = [
        // `agent.activity.query`, first because it sorts first by `builtin_key`.
        "tin_01ARZ3NDEKTSV4RRFFQ69G5FB5",
        "tin_01ARZ3NDEKTSV4RRFFQ69G5FB0",
        "tin_01ARZ3NDEKTSV4RRFFQ69G5FB1",
        "tin_01ARZ3NDEKTSV4RRFFQ69G5FB2",
        "tin_01ARZ3NDEKTSV4RRFFQ69G5FB3",
        "tin_01ARZ3NDEKTSV4RRFFQ69G5FB4",
    ];
    if definitions.len() != instance_ids.len() {
        return Err(BuiltinToolDefinitionSetError::DefinitionMismatch);
    }
    let created_at: UtcTimestamp = "2026-09-06T00:00:00Z"
        .parse()
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    definitions
        .into_iter()
        .zip(instance_ids)
        .map(|(definition, instance_id)| {
            let handler = BuiltinCoreHandlerV1::all()
                .iter()
                .copied()
                .find(|handler| handler.builtin_key() == definition.builtin_key.as_str())
                .ok_or(BuiltinToolDefinitionSetError::DefinitionMismatch)?;
            let execution_binding = ToolInstanceExecutionBindingV1::BuiltInCoreHandler { handler };
            let record = build_definition_record(&definition, handler, created_at)?;
            let tool_instance_id =
                instance_id.parse().map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
            let tool_instance_revision = Revision::INITIAL;
            let fingerprint = canonical_sha256(&BuiltinSourceContractFingerprintMaterialV1 {
                tool_id: definition.tool_id,
                tool_version: &definition.tool_version,
                tool_instance_id,
                tool_instance_revision,
                definition_fingerprint: &record.definition_fingerprint,
                execution_binding,
            })
            .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
            Ok(SignedBuiltinReleaseEntryV1 {
                definition,
                tool_instance_id,
                tool_instance_revision,
                definition_fingerprint: record.definition_fingerprint,
                source_contract_fingerprint: SignedBuiltinToolContractFingerprintV1::new(
                    fingerprint,
                ),
                execution_binding,
            })
        })
        .collect()
}

pub(super) fn build_builtin_definitions()
-> Result<Vec<BuiltinToolDefinitionV1>, BuiltinToolDefinitionSetError> {
    // R4: entries must be ordered by `builtin_key`, and `agent.activity.query` sorts **before**
    // every `tools.*` key, so it is first rather than appended. The instance-id array, the Tool ID
    // Registry and this list are all read positionally, so all three carry the same order.
    let ids = [
        // R4: the model-executable run-information query.
        ("agent.activity.query", "tol_01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        ("tools.describe", "tol_01ARZ3NDEKTSV4RRFFQ69G5FAV"),
        ("tools.list_loaded", "tol_01ARZ3NDEKTSV4RRFFQ69G5FAW"),
        ("tools.load", "tol_01ARZ3NDEKTSV4RRFFQ69G5FAX"),
        ("tools.search", "tol_01ARZ3NDEKTSV4RRFFQ69G5FAY"),
        ("tools.unload", "tol_01ARZ3NDEKTSV4RRFFQ69G5FAZ"),
    ];
    let version = ToolVersion::new(None, Revision::INITIAL)
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    let mut definitions = Vec::with_capacity(ids.len());
    for (name, id) in ids {
        let (summary, description, input_schema, result_schema, capabilities, scopes) = match name {
            "tools.describe" => (
                TOOLS_DESCRIBE_MODEL_SUMMARY_V1,
                TOOLS_DESCRIBE_MODEL_DESCRIPTION_V1,
                schema_value::<ToolDescribeRequestV1>()?,
                schema_value::<ToolDescribeResultV1>()?,
                vec![BuiltinToolCapabilityV1::ToolRegistryRead],
                vec![BuiltinToolScopeRequirementV1::ToolRegistrySnapshot],
            ),
            "tools.list_loaded" => (
                TOOLS_LIST_LOADED_MODEL_SUMMARY_V1,
                TOOLS_LIST_LOADED_MODEL_DESCRIPTION_V1,
                schema_value::<ToolListLoadedRequestV1>()?,
                schema_value::<ToolListLoadedResultV1>()?,
                vec![BuiltinToolCapabilityV1::ToolRegistryRead],
                vec![BuiltinToolScopeRequirementV1::ToolRegistrySnapshot],
            ),
            "tools.load" => (
                TOOLS_LOAD_MODEL_SUMMARY_V1,
                TOOLS_LOAD_MODEL_DESCRIPTION_V1,
                schema_value::<ToolLoadRequestV1>()?,
                schema_value::<ToolLoadResultV1>()?,
                vec![
                    BuiltinToolCapabilityV1::ToolRegistryRead,
                    BuiltinToolCapabilityV1::ToolEphemeralExposureWrite,
                ],
                vec![
                    BuiltinToolScopeRequirementV1::ToolRegistrySnapshot,
                    BuiltinToolScopeRequirementV1::TurnTaskToolExposure,
                ],
            ),
            "tools.search" => (
                TOOLS_SEARCH_MODEL_SUMMARY_V1,
                TOOLS_SEARCH_MODEL_DESCRIPTION_V1,
                schema_value::<ToolSearchRequestV1>()?,
                schema_value::<ToolSearchPageV1>()?,
                vec![BuiltinToolCapabilityV1::ToolRegistryRead],
                vec![BuiltinToolScopeRequirementV1::ToolRegistrySnapshot],
            ),
            "tools.unload" => (
                TOOLS_UNLOAD_MODEL_SUMMARY_V1,
                TOOLS_UNLOAD_MODEL_DESCRIPTION_V1,
                schema_value::<ToolLoadRequestV1>()?,
                schema_value::<ToolLoadResultV1>()?,
                vec![
                    BuiltinToolCapabilityV1::ToolRegistryRead,
                    BuiltinToolCapabilityV1::ToolEphemeralExposureWrite,
                ],
                vec![
                    BuiltinToolScopeRequirementV1::ToolRegistrySnapshot,
                    BuiltinToolScopeRequirementV1::TurnTaskToolExposure,
                ],
            ),
            // R4: the run-information query needs **no resource scope**, because it has no effect
            // on a resource. Its only requirement is the registry snapshot, which is the ordinary
            // "this call is being judged against a frozen registry" fact every builtin carries -
            // asking what is running must not require a claim on the thing being asked about, or the
            // question would fail exactly when the resource is busy, which is when it matters most.
            "agent.activity.query" => (
                aura_activity_domain::AGENT_ACTIVITY_QUERY_MODEL_SUMMARY_V1,
                aura_activity_domain::AGENT_ACTIVITY_QUERY_MODEL_DESCRIPTION_V1,
                schema_value::<aura_activity_domain::AgentActivityQueryInput>()?,
                schema_value::<aura_activity_domain::AgentActivityQueryResultV1>()?,
                vec![BuiltinToolCapabilityV1::ToolRegistryRead],
                vec![BuiltinToolScopeRequirementV1::ToolRegistrySnapshot],
            ),
            _ => return Err(BuiltinToolDefinitionSetError::DefinitionMismatch),
        };
        definitions.push(BuiltinToolDefinitionV1 {
            builtin_key: name
                .parse()
                .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
            tool_id: id.parse().map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
            tool_version: version.clone(),
            model_summary: summary.to_owned(),
            model_description: description.to_owned(),
            strict: true,
            source: ToolSourceKindV1::BuiltIn,
            audience: ToolAudienceV1::ChildRuntimeRequired,
            capabilities,
            scope_requirements: scopes,
            input_schema: ToolInputSchemaV1::new(input_schema)
                .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
            result_contract: ToolResultContractV1::StructuredJson {
                result_schema: ToolStructuredResultSchemaV1::new(result_schema)
                    .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?,
            },
        });
    }
    Ok(definitions)
}

pub(super) fn build_definition_record(
    definition: &BuiltinToolDefinitionV1,
    handler: BuiltinCoreHandlerV1,
    created_at: UtcTimestamp,
) -> Result<ToolDefinitionRecordV1, BuiltinToolDefinitionSetError> {
    let capabilities = definition
        .capabilities
        .iter()
        .map(|capability| match capability {
            BuiltinToolCapabilityV1::ToolRegistryRead => ResolvedCapability::ToolRegistryRead,
            BuiltinToolCapabilityV1::ToolEphemeralExposureWrite => {
                ResolvedCapability::ToolEphemeralExposureWrite
            }
        })
        .collect::<Vec<_>>();
    let may_create_persistent_effects =
        matches!(handler, BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload);
    let source_contract_key = match handler {
        BuiltinCoreHandlerV1::ToolsSearch => "aura.builtin.tools.search.v1",
        BuiltinCoreHandlerV1::ToolsDescribe => "aura.builtin.tools.describe.v1",
        BuiltinCoreHandlerV1::ToolsLoad => "aura.builtin.tools.load.v1",
        BuiltinCoreHandlerV1::ToolsUnload => "aura.builtin.tools.unload.v1",
        BuiltinCoreHandlerV1::ToolsListLoaded => "aura.builtin.tools.list_loaded.v1",
        BuiltinCoreHandlerV1::ActivityQuery => "aura.builtin.agent.activity_query.v1",
    };
    let display_name = match handler {
        BuiltinCoreHandlerV1::ToolsSearch => "Search tools",
        BuiltinCoreHandlerV1::ToolsDescribe => "Describe tool",
        BuiltinCoreHandlerV1::ToolsLoad => "Load tools",
        BuiltinCoreHandlerV1::ToolsUnload => "Unload tools",
        BuiltinCoreHandlerV1::ToolsListLoaded => "List loaded tools",
        BuiltinCoreHandlerV1::ActivityQuery => "Query running activity",
    };
    let runtime = ToolRuntimeDescriptorV1::ExternalSource {
        source_contract_key: source_contract_key.to_owned(),
    };
    let runtime_file_closure_hash =
        canonical_sha256(&("builtin_core_handler_v1", source_contract_key, handler))
            .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    ToolDefinitionRecordV1::new(
        definition.tool_id,
        definition.tool_version.clone(),
        None,
        None,
        None,
        definition.builtin_key.clone(),
        display_name.to_owned(),
        definition.model_description.clone(),
        definition.model_summary.clone(),
        definition.input_schema.clone(),
        definition.strict,
        definition.result_contract.clone(),
        runtime,
        capabilities,
        Vec::new(),
        definition.audience,
        ToolSourceKindV1::BuiltIn,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed: false,
            repeat_is_idempotent: true,
            may_create_persistent_effects,
        },
        runtime_file_closure_hash,
        definition.tool_version.definition_revision,
        created_at,
    )
    .map_err(|_| BuiltinToolDefinitionSetError::DefinitionMismatch)
}

pub(crate) fn schema_value<T: JsonSchema>()
-> Result<serde_json::Value, BuiltinToolDefinitionSetError> {
    let mut value = serde_json::to_value(schema_for!(T))
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    // `$id` is deliberately not emitted by schemars here. The official draft
    // marker is accepted by Canonical Tool Schema V1 and local refs remain in
    // the same document.
    if let Some(root) = value.as_object_mut() {
        root.remove("title");
    }
    remove_lossless_rust_integer_formats(&mut value)?;
    Ok(value)
}

pub(super) fn remove_lossless_rust_integer_formats(
    value: &mut serde_json::Value,
) -> Result<(), BuiltinToolDefinitionSetError> {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                remove_lossless_rust_integer_formats(value)?;
            }
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                remove_lossless_rust_integer_formats(value)?;
            }
            // R4: `default` describes **deserialization**, not validation, and Canonical Tool Schema
            // V1 does not allow it. A Rust field carrying `#[serde(default)]` - the activity query's
            // optional `offset`/`limit`, for example - emits one, so it is removed here, exactly as
            // `agent_release::schema` already removed it on the management tools' path. Omitting it
            // does not make the property required or change the Rust semantics: the property simply
            // stays optional with no advertised default.
            object.remove("default");
            let format = object.get("format").and_then(serde_json::Value::as_str);
            // R4: `uint64` and `int64` are handled here too.
            //
            // They were missing, and the omission was invisible until a builtin definition used a
            // type that emits one (`Revision`, which the activity query's `offset`/`limit` and the
            // page's `revision` all carry). The model-facing surface uses interoperable integers -
            // a JSON number is only losslessly exact up to 2^53 - so a 64-bit value is bounded to
            // that exact range and its `format` is dropped. `agent_release::schema` already did
            // this; this copy did not, so the same Rust type produced a canonical-schema failure on
            // one path and a valid schema on the other. Both now apply the same rule.
            let bounds: Option<(i64, i64)> = match format {
                Some("uint8") => Some((0, i64::from(u8::MAX))),
                Some("uint16") => Some((0, i64::from(u16::MAX))),
                Some("uint32") => Some((0, i64::from(u32::MAX))),
                Some("int32") => Some((i64::from(i32::MIN), i64::from(i32::MAX))),
                Some("uint64") => Some((0, 9_007_199_254_740_991)),
                Some("int64") => Some((-9_007_199_254_740_991, 9_007_199_254_740_991)),
                _ => None,
            };
            if let Some((minimum, maximum)) = bounds {
                let is_integer =
                    object.get("type").and_then(serde_json::Value::as_str) == Some("integer");
                let declared_minimum = object.get("minimum").and_then(serde_json::Value::as_i64);
                let declared_maximum = object.get("maximum").and_then(serde_json::Value::as_i64);
                let preserves_native_bounds = declared_minimum.is_none_or(|value| value >= minimum)
                    && declared_maximum.is_none_or(|value| value <= maximum);
                if !is_integer || !preserves_native_bounds {
                    return Err(BuiltinToolDefinitionSetError::InvalidDocument);
                }
                object.entry("minimum").or_insert_with(|| serde_json::Value::from(minimum));
                object.entry("maximum").or_insert_with(|| serde_json::Value::from(maximum));
                object.remove("format");
            }
        }
        _ => {}
    }
    Ok(())
}
