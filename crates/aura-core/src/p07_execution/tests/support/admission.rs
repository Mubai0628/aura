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
    pub(super) registry: ToolRegistrySnapshotV1,
    pub(super) definition: ToolDefinitionRecordV1,
    pub(super) instance: ToolInstanceRecordV1,
    pub(super) invocation: aura_tool_contract::CoreInvocationIdentityV1,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let timestamp = UtcTimestamp::now();
        let source_contract_fingerprint = ToolSourceContractFingerprintV1::BuiltIn {
            signed_builtin_tool_contract_fingerprint:
                aura_tool_contract::SignedBuiltinToolContractFingerprintV1::new(
                    CanonicalHashV1::sha256(b"builtin source"),
                ),
        };
        let definition = ToolDefinitionRecordV1::new(
            ToolId::new(),
            ToolVersion::new(None, Revision::INITIAL).expect("version"),
            None,
            None,
            None,
            ToolCanonicalName::new("fixture.execute").expect("name"),
            "Fixture".to_owned(),
            "Execute the fixture with one string value.".to_owned(),
            "Execute a fixture".to_owned(),
            ToolInputSchemaV1::new(json!({
                "type": "object",
                "properties": {"value": {"type": "string"}},
                "required": ["value"],
                "additionalProperties": false
            }))
            .expect("schema"),
            true,
            ToolResultContractV1::Unit,
            ToolRuntimeDescriptorV1::ExternalSource {
                source_contract_key: "fixture.execute.v1".to_owned(),
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
            CanonicalHashV1::sha256(b"runtime"),
            Revision::INITIAL,
            timestamp,
        )
        .expect("definition");
        let instance = ToolInstanceRecordV1 {
            tool_instance_id: ToolInstanceId::new(),
            tool_id: definition.tool_id,
            tool_version: definition.tool_version.clone(),
            source_kind: ToolSourceKindV1::BuiltIn,
            source_contract_fingerprint: source_contract_fingerprint.clone(),
            enabled: true,
            runtime_health: ToolRuntimeHealthV1::Healthy,
            overrides: ToolInstanceOverridesV1 {
                timeout_ms: None,
                max_retries: None,
                max_concurrent_attempts: None,
                subagent_enabled: None,
            },
            entity_revision: Revision::INITIAL,
            created_at: timestamp,
            updated_at: timestamp,
        };
        let invocation = aura_tool_contract::CoreInvocationIdentityV1 {
            command_invocation_id: CommandInvocationId::new(),
            session_id: SessionId::new(),
            turn_task_id: TurnTaskId::new(),
            agent_run_id: AgentRunId::new(),
            model_step_id: ModelStepId::new(),
        };
        let schema = ToolSchemaSnapshotV1::new(
            definition.tool_id,
            definition.tool_version.clone(),
            definition.canonical_name.clone(),
            definition.model_description.clone(),
            definition.input_schema.clone(),
            definition.strict,
        )
        .expect("schema snapshot");
        let catalog = ToolRegistryCatalogEntryV1 {
            tool_id: definition.tool_id,
            tool_version: definition.tool_version.clone(),
            tool_instance_id: instance.tool_instance_id,
            canonical_name: definition.canonical_name.clone(),
            display_name: definition.display_name.clone(),
            summary: definition.model_summary.clone(),
            source: ToolSourceKindV1::BuiltIn,
            definition_fingerprint: definition.definition_fingerprint.clone(),
            source_contract_fingerprint: source_contract_fingerprint.clone(),
            load_state: ToolLoadStateV1::Ephemeral,
        };
        let binding = EffectiveToolBindingSnapshot {
            tool_id: definition.tool_id,
            tool_version: definition.tool_version.clone(),
            tool_instance_id: instance.tool_instance_id,
            source_kind: ToolSourceKindV1::BuiltIn,
            exposure: ToolExposureV1::EphemeralLoad,
            definition_fingerprint: definition.definition_fingerprint.clone(),
            source_contract_fingerprint,
        };
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
            vec![schema],
            vec![catalog],
            vec![binding],
            timestamp,
        )
        .expect("registry");
        Self { registry, definition, instance, invocation }
    }

    pub(super) fn call(&self, arguments: serde_json::Value) -> NormalizedToolCallV1 {
        NormalizedToolCallV1 {
            tool_call_id: ToolCallId::new(),
            ordinal: 1,
            definition_ref: Some(ToolDefinitionSnapshotRef {
                tool_id: self.definition.tool_id,
                tool_version: self.definition.tool_version.clone(),
                registry_snapshot_ref: self.registry.snapshot_ref.clone(),
            }),
            input: NormalizedToolCallInputStateV1::Ready {
                purpose: "Exercise the fixture".parse().expect("purpose"),
                arguments_hash: canonical_sha256(&arguments).expect("arguments hash"),
                arguments,
            },
            provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"provider envelope"),
        }
    }

    pub(super) fn batch(&self, calls: Vec<NormalizedToolCallV1>) -> ToolBatchRequestV1 {
        ToolBatchRequestV1::new(
            ToolBatchId::new(),
            self.invocation.clone(),
            Revision::INITIAL,
            CanonicalHashV1::sha256(b"control state"),
            self.registry.snapshot_ref.clone(),
            calls,
        )
        .expect("batch")
    }

    pub(super) fn prepare(
        &self,
        batch: ToolBatchRequestV1,
        planning_inputs: Vec<FrozenToolCallPlanningInputV1>,
    ) -> ToolBatchPreparationOutcomeV1 {
        ToolBatchAdmissionServiceV1
            .prepare(PrepareToolBatchExecutionInputV1 {
                batch,
                registry: &self.registry,
                definitions: vec![self.definition.clone()],
                admission_authority: &AllowAuthority,
                settings: &ToolSettingsSnapshotV1::default(),
                planning_inputs,
                admitted_at: UtcTimestamp::now(),
                correlation_id: CorrelationId::new(),
                causation_id: None,
            })
            .expect("prepare batch")
    }
}
