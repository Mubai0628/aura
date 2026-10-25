use super::*;
use aura_domain_common::jcs::canonical_json_jcs;

#[test]
fn embedded_registry_has_stable_unique_builtin_tool_ids() {
    let registry = embedded_builtin_tool_id_registry_v1().expect("builtin registry");
    // R4: six builtins. `agent.activity.query` is the model-executable run-information query,
    // and it sorts **before** every `tools.*` key, which is why it is the first entry rather
    // than an appended one.
    assert_eq!(registry.entries().len(), 6);
    assert_eq!(
        registry.entries().first().expect("an entry").builtin_key.as_str(),
        "agent.activity.query"
    );
    assert_eq!(
        registry.tool_id("tools.search").expect("tools.search").to_string(),
        "tol_01ARZ3NDEKTSV4RRFFQ69G5FAY"
    );
    assert_eq!(
        registry.tool_id("agent.activity.query").expect("agent.activity.query").to_string(),
        "tol_01ARZ3NDEKTSV4RRFFQ69G5FB0"
    );
    assert_eq!(registry.source_hash().hex, BUILTIN_TOOL_ID_REGISTRY_SOURCE_SHA256_V1);
    assert_eq!(registry.canonical_hash().algorithm().to_string(), "sha256");
}

#[test]
fn embedded_definition_set_matches_ids_contracts_and_release_source() {
    let set = embedded_builtin_tool_definition_set_v1().expect("definition set");
    assert_eq!(set.definitions().len(), 6);
    assert!(set.definitions().iter().all(|definition| {
        definition.strict
            && definition.source == ToolSourceKindV1::BuiltIn
            && definition.audience == ToolAudienceV1::ChildRuntimeRequired
    }));
    assert_eq!(
        set.definition("tools.search").expect("tools.search").tool_id.to_string(),
        "tol_01ARZ3NDEKTSV4RRFFQ69G5FAY"
    );
    assert!(
        !canonical_json_jcs(&BuiltinToolDefinitionSetWireV1 {
            schema_version: BUILTIN_TOOL_DEFINITION_SET_SCHEMA_VERSION_V1,
            release_revision: Revision::INITIAL,
            entries: set.entries().to_vec(),
        })
        .expect("canonical set")
        .is_empty()
    );
}

#[test]
fn duplicate_keys_are_rejected_before_typed_deserialization() {
    let source = br#"{"schema_version":1,"schema_version":1,"entries":[]}"#;
    assert_eq!(
        parse_registry(source, ContentHash::sha256(source)),
        Err(BuiltinToolIdRegistryError::InvalidDocument)
    );
}

#[test]
fn generated_builtin_schemas_fit_the_canonical_local_subset() {
    let cases = [
        schema_value::<ToolSearchRequestV1>(),
        schema_value::<ToolSearchPageV1>(),
        schema_value::<ToolDescribeRequestV1>(),
        schema_value::<ToolDescribeResultV1>(),
        schema_value::<ToolLoadRequestV1>(),
        schema_value::<ToolLoadResultV1>(),
        schema_value::<ToolListLoadedRequestV1>(),
        schema_value::<ToolListLoadedResultV1>(),
    ];
    for schema in cases {
        let schema = schema.expect("schemars");
        ToolInputSchemaV1::new(schema.clone()).expect("input schema");
        ToolStructuredResultSchemaV1::new(schema).expect("result schema");
    }
}

#[test]
fn signed_definitions_resolve_one_exact_closed_handler_each() {
    let set = embedded_builtin_tool_definition_set_v1().expect("definition set");
    let handlers = set
        .entries()
        .iter()
        .map(|entry| {
            set.handler(entry.definition.tool_id, &entry.definition.tool_version)
                .expect("exact handler")
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(handlers, BuiltinCoreHandlerV1::all().iter().copied().collect());

    let wrong_version = ToolVersion::new(None, Revision::new(2)).expect("different version");
    assert_eq!(
        set.handler(set.definitions()[0].tool_id, &wrong_version),
        Err(BuiltinToolDefinitionSetError::DefinitionMismatch)
    );
}

#[test]
fn signed_release_fixes_stable_instances_fingerprints_and_semantics() {
    let entries = build_builtin_release_entries().expect("release entries");
    assert_eq!(entries.len(), 6);
    // R4: position 0 is `agent.activity.query`, because entries are ordered by `builtin_key`.
    assert_eq!(entries[0].definition.builtin_key.as_str(), "agent.activity.query");
    assert_eq!(entries[0].tool_instance_id.to_string(), "tin_01ARZ3NDEKTSV4RRFFQ69G5FB5");
    assert_eq!(entries[1].tool_instance_id.to_string(), "tin_01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let created_at: UtcTimestamp = "2026-09-06T01:00:00Z".parse().expect("timestamp");
    for entry in entries {
        let definition = entry.definition_record(created_at).expect("definition record");
        let instance = entry.instance_record(created_at);
        assert_eq!(definition.definition_fingerprint, entry.definition_fingerprint);
        assert_eq!(instance.tool_instance_id, entry.tool_instance_id);
        assert!(instance.enabled);
        assert_eq!(instance.runtime_health, ToolRuntimeHealthV1::Healthy);
        assert!(!definition.execution_semantics.automatic_retry_allowed);
        assert!(definition.execution_semantics.repeat_is_idempotent);
        assert_eq!(
            definition.execution_semantics.may_create_persistent_effects,
            matches!(
                entry.execution_binding.builtin_handler(),
                BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload
            )
        );
    }
}
