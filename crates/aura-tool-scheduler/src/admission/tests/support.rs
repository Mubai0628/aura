use super::*;

pub(super) struct AllowAuthority;

impl BatchAdmissionAuthorityV1 for AllowAuthority {
    fn verify_batch_admission(
        &self,
        _batch: &ToolBatchRequestV1,
    ) -> Result<(), BatchAdmissionAuthorityIssueV1> {
        Ok(())
    }
}

pub(super) struct Fixture {
    pub(super) invocation: aura_tool_contract::CoreInvocationIdentityV1,
    pub(super) registry: ToolRegistrySnapshotV1,
    pub(super) definitions: Vec<ToolDefinitionRecordV1>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Self::with_definitions(vec![definition(1, "sample.first"), definition(2, "sample.second")])
    }

    /// A fixture over an explicit definition set, so a test can compare two tools
    /// whose schemas disagree instead of only renaming one shape.
    pub(super) fn with_definitions(definitions: Vec<ToolDefinitionRecordV1>) -> Self {
        let invocation = aura_tool_contract::CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::new(),
            session_id: SessionId::new(),
            turn_task_id: TurnTaskId::new(),
            agent_run_id: AgentRunId::new(),
            model_step_id: ModelStepId::new(),
        };
        let source_contract_fingerprint =
            aura_tool_contract::ToolSourceContractFingerprintV1::BuiltIn {
                signed_builtin_tool_contract_fingerprint:
                    aura_tool_contract::SignedBuiltinToolContractFingerprintV1::new(
                        CanonicalHashV1::sha256(b"source-contract"),
                    ),
            };
        let mut schemas = Vec::new();
        let mut catalog = Vec::new();
        let mut bindings = Vec::new();
        for (index, definition) in definitions.iter().enumerate() {
            let instance_id = ToolInstanceId::from_bytes(
                [u8::try_from(index + 10).expect("fixture index fits in one byte"); 16],
            );
            schemas.push(
                ToolSchemaSnapshotV1::new(
                    definition.tool_id,
                    definition.tool_version.clone(),
                    definition.canonical_name.clone(),
                    definition.model_description.clone(),
                    definition.input_schema.clone(),
                    definition.strict,
                )
                .expect("schema snapshot"),
            );
            catalog.push(ToolRegistryCatalogEntryV1 {
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                tool_instance_id: instance_id,
                canonical_name: definition.canonical_name.clone(),
                display_name: definition.display_name.clone(),
                summary: definition.model_summary.clone(),
                source: definition.source_kind,
                definition_fingerprint: definition.definition_fingerprint.clone(),
                source_contract_fingerprint: source_contract_fingerprint.clone(),
                load_state: ToolLoadStateV1::Ephemeral,
            });
            bindings.push(EffectiveToolBindingSnapshot {
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                tool_instance_id: instance_id,
                source_kind: definition.source_kind,
                exposure: ToolExposureV1::EphemeralLoad,
                definition_fingerprint: definition.definition_fingerprint.clone(),
                source_contract_fingerprint: source_contract_fingerprint.clone(),
            });
        }
        let registry = ToolRegistrySnapshotV1::new(
            ToolRegistrySnapshotId::new(),
            Revision::INITIAL,
            ToolRegistryScopeV1::Session {
                workspace_id: WorkspaceId::new(),
                session_id: invocation.session_id,
                turn_task_id: invocation.turn_task_id,
                agent_run_id: invocation.agent_run_id,
            },
            None,
            None,
            CanonicalHashV1::sha256(b"eligibility"),
            schemas,
            catalog,
            bindings,
            UtcTimestamp::now(),
        )
        .expect("registry snapshot");
        Self { invocation, registry, definitions }
    }

    pub(super) fn call(
        &self,
        definition_index: usize,
        ordinal: u32,
        input: NormalizedToolCallInputStateV1,
    ) -> NormalizedToolCallV1 {
        let definition = &self.definitions[definition_index];
        NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new(),
            ordinal,
            definition_ref: Some(ToolDefinitionSnapshotRef {
                tool_id: definition.tool_id,
                tool_version: definition.tool_version.clone(),
                registry_snapshot_ref: self.registry.snapshot_ref.clone(),
            }),
            input,
            provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"provider-envelope"),
        }
    }

    pub(super) fn batch(&self, calls: Vec<NormalizedToolCallV1>) -> ToolBatchRequestV1 {
        ToolBatchRequestV1::new(
            ToolBatchId::new(),
            self.invocation.clone(),
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"p08-state"),
            self.registry.snapshot_ref.clone(),
            calls,
        )
        .expect("batch")
    }
}

pub(super) fn definition(byte: u8, name: &str) -> ToolDefinitionRecordV1 {
    definition_with_schema(
        byte,
        name,
        json!({
            "type": "object",
            "properties": {"value": {"type": "string"}},
            "required": ["value"],
            "additionalProperties": false
        }),
    )
}

pub(super) fn definition_with_schema(
    byte: u8,
    name: &str,
    schema: Value,
) -> ToolDefinitionRecordV1 {
    ToolDefinitionRecordV1::new(
        ToolId::from_bytes([byte; 16]),
        ToolVersion::new(None, Revision::INITIAL).expect("tool version"),
        None,
        None,
        None,
        ToolCanonicalName::new(name).expect("canonical name"),
        name.to_owned(),
        format!("Execute {name}"),
        format!("Short summary for {name}"),
        ToolInputSchemaV1::new(schema).expect("input schema"),
        true,
        ToolResultContractV1::Unit,
        ToolRuntimeDescriptorV1::ExternalSource {
            source_contract_key: format!("fixture.{name}.v1"),
        },
        Vec::new(),
        Vec::new(),
        ToolAudienceV1::SubagentConfigurable,
        ToolSourceKindV1::BuiltIn,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed: false,
            repeat_is_idempotent: true,
            may_create_persistent_effects: false,
        },
        CanonicalHashV1::sha256(format!("runtime-{name}").as_bytes()),
        Revision::INITIAL,
        UtcTimestamp::now(),
    )
    .expect("definition")
}

pub(super) fn ready(arguments: Value) -> NormalizedToolCallInputStateV1 {
    let arguments_hash = canonical_sha256(&arguments).expect("arguments hash");
    NormalizedToolCallInputStateV1::Ready {
        purpose: "Use this fixture".parse().expect("purpose"),
        arguments,
        arguments_hash,
    }
}
