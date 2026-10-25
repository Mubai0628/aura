//! The unified entry: a discoverable tool reached through `tools.call` is admitted with the
//! registry's entry binding, and nothing the registry did not offer is admitted or authorized.
use super::*;

struct EntryFixture {
    base: Fixture,
}

impl EntryFixture {
    /// `sample.sent` and `tools.call` are sent; `sample.found` is offered through the entry;
    /// `goal.complete` is discoverable but never offered.
    fn new() -> Self {
        let definitions = vec![
            definition(1, "sample.sent"),
            definition(2, "sample.found"),
            definition(3, "goal.complete"),
            definition_with_schema(
                4,
                "tools.call",
                json!({
                    "type": "object",
                    "properties": {
                        "tool": {"type": "string"},
                        "arguments": {"type": "object", "additionalProperties": true}
                    },
                    "required": ["tool", "arguments"],
                    "additionalProperties": false
                }),
            ),
        ];
        let mut base = Fixture::with_definitions(definitions.clone());
        let source = base.registry.effective_bindings[0].source_contract_fingerprint.clone();
        let mut schemas = Vec::new();
        let mut catalog = Vec::new();
        let mut effective = Vec::new();
        let mut entry = Vec::new();
        for (index, definition) in definitions.iter().enumerate() {
            let instance =
                ToolInstanceId::from_bytes([u8::try_from(index + 10).expect("byte"); 16]);
            let sent = matches!(definition.canonical_name.as_str(), "sample.sent" | "tools.call");
            catalog.push(ToolRegistryCatalogEntryV1 {
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                tool_instance_id: instance,
                canonical_name: definition.canonical_name.clone(),
                display_name: definition.display_name.clone(),
                summary: definition.model_summary.clone(),
                source: definition.source_kind,
                definition_fingerprint: definition.definition_fingerprint.clone(),
                source_contract_fingerprint: source.clone(),
                load_state: if sent {
                    ToolLoadStateV1::Core
                } else {
                    ToolLoadStateV1::Discoverable
                },
            });
            let binding = |exposure| EffectiveToolBindingSnapshot {
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                tool_instance_id: instance,
                source_kind: definition.source_kind,
                exposure,
                definition_fingerprint: definition.definition_fingerprint.clone(),
                source_contract_fingerprint: source.clone(),
            };
            if sent {
                effective.push(binding(ToolExposureV1::Core));
                schemas.push(
                    ToolSchemaSnapshotV1::new(
                        definition.tool_id,
                        definition.tool_version.clone(),
                        definition.canonical_name.clone(),
                        definition.model_description.clone(),
                        definition.input_schema.clone(),
                        definition.strict,
                    )
                    .expect("schema"),
                );
            } else if definition.canonical_name.as_str() == "sample.found" {
                entry.push(binding(ToolExposureV1::UnifiedEntry));
            }
        }
        base.registry = ToolRegistrySnapshotV1::with_entry_bindings(
            ToolRegistrySnapshotId::new(),
            Revision::INITIAL,
            base.registry.scope.clone(),
            None,
            None,
            CanonicalHashV1::sha256(b"eligibility"),
            schemas,
            catalog,
            effective,
            entry,
            UtcTimestamp::now(),
        )
        .expect("registry with an entry");
        Self { base }
    }

    fn index(&self, name: &str) -> usize {
        self.base
            .definitions
            .iter()
            .position(|d| d.canonical_name.as_str() == name)
            .expect("fixture tool")
    }

    /// The call the model made through the entry, rewritten as every reader rewrites it.
    fn through_entry(&self, arguments: Value) -> NormalizedToolCallV1 {
        let mut calls = vec![self.base.call(self.index("tools.call"), 0, ready(arguments))];
        aura_tool_contract::expand_entry_calls(&self.base.registry, &mut calls);
        calls.remove(0)
    }

    fn admit(&self, calls: Vec<NormalizedToolCallV1>) -> AdmittedToolBatchV1 {
        let batch = self.base.batch(calls);
        match admit_tool_batch(
            batch,
            &self.base.registry,
            self.base.definitions.clone(),
            &AllowAuthority,
        )
        .expect("admission")
        {
            ToolBatchAdmissionOutcomeV1::Admitted(admitted) => *admitted,
            other => panic!("admitted batch expected: {other:?}"),
        }
    }
}

#[test]
fn a_call_through_the_entry_is_admitted_as_the_named_tool_with_its_entry_binding() {
    let f = EntryFixture::new();
    let call = f.through_entry(json!({"tool": "sample.found", "arguments": {"value": "x"}}));
    let found = &f.base.definitions[f.index("sample.found")];
    assert_eq!(call.definition_ref.as_ref().map(|r| r.tool_id), Some(found.tool_id));
    let admitted = f.admit(vec![call.clone()]);
    assert!(admitted.not_executed.is_empty(), "{:?}", admitted.not_executed);
    let validated = &admitted.validated_calls[0];
    assert_eq!(validated.call, call);
    assert_eq!(validated.definition.tool_id, found.tool_id);
    assert_eq!(validated.binding.exposure, ToolExposureV1::UnifiedEntry);
    security_source_for_admitted_tool_call_v1(validated, &f.base.registry)
        .expect("the entry binding authorizes exactly this tool");
    // The named tool's own schema decides its arguments.
    let wrong = f.through_entry(json!({"tool": "sample.found", "arguments": {"value": 1}}));
    let admitted = f.admit(vec![wrong]);
    assert!(admitted.validated_calls.is_empty());
    assert_eq!(admitted.not_executed[0].reason, ToolNotExecutedReasonV1::SchemaValidationFailed);
}

#[test]
fn nothing_the_registry_did_not_offer_is_admitted_through_or_around_the_entry() {
    let f = EntryFixture::new();
    // A discoverable tool the registry does not offer cannot be named directly...
    let direct = f.base.call(f.index("goal.complete"), 0, ready(json!({"value": "x"})));
    let admitted = f.admit(vec![direct]);
    assert_eq!(admitted.not_executed[0].reason, ToolNotExecutedReasonV1::NotLoaded);
    // ...nor through the entry, which then stays the entry's call and runs nothing.
    let entry = f.through_entry(json!({"tool": "goal.complete", "arguments": {"value": "x"}}));
    assert!(aura_tool_contract::is_entry_call(&f.base.registry, &entry));
    let mut unknown = f.through_entry(json!({"tool": "sample.none", "arguments": {}}));
    unknown.ordinal = 1;
    let admitted = f.admit(vec![entry, unknown]);
    assert!(admitted.validated_calls.is_empty());
    for rejection in &admitted.not_executed {
        assert_eq!(rejection.reason, ToolNotExecutedReasonV1::ToolNotFound);
    }
    // Input that is not {tool, arguments} is the entry's own schema error.
    let malformed = f.through_entry(json!({"tool": "sample.found"}));
    let admitted = f.admit(vec![malformed]);
    assert_eq!(admitted.not_executed[0].reason, ToolNotExecutedReasonV1::SchemaValidationFailed);
}

#[test]
fn an_entry_binding_authorizes_only_the_tool_the_registry_offered_through_the_entry() {
    let f = EntryFixture::new();
    let sent = f.base.call(f.index("sample.sent"), 0, ready(json!({"value": "x"})));
    let admitted = f.admit(vec![sent]);
    let mut forged = admitted.validated_calls[0].clone();
    assert_eq!(forged.binding.exposure, ToolExposureV1::Core);
    security_source_for_admitted_tool_call_v1(&forged, &f.base.registry).expect("sent tool");
    // A sent tool claiming the entry's exposure has no entry binding behind it.
    forged.binding.exposure = ToolExposureV1::UnifiedEntry;
    assert!(security_source_for_admitted_tool_call_v1(&forged, &f.base.registry).is_err());
    // An entry call claiming to be sent has no sent binding behind it.
    let call = f.through_entry(json!({"tool": "sample.found", "arguments": {"value": "x"}}));
    let mut forged = f.admit(vec![call]).validated_calls[0].clone();
    forged.binding.exposure = ToolExposureV1::EphemeralLoad;
    assert!(security_source_for_admitted_tool_call_v1(&forged, &f.base.registry).is_err());
}
