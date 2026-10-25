use super::*;

pub(super) struct BuiltinVerticalFixture {
    pub(super) _root: TempDir,
    pub(super) database_path: PathBuf,
    pub(super) persistence: PersistenceCoordinator,
    pub(super) scope: ToolRegistryScopeV1,
    pub(super) workspace_id: WorkspaceId,
    pub(super) session_id: SessionId,
    pub(super) turn_task_id: TurnTaskId,
    pub(super) agent_run_id: AgentRunId,
    pub(super) permission_snapshot: aura_security_contract::PermissionSnapshot,
    pub(super) external_candidate: RegistryCandidateV1,
    pub(super) initial_registry_snapshot_ref: ToolRegistrySnapshotRef,
}

pub(super) struct BuiltinExecutionObservationV1 {
    pub(super) result: ToolResultV1,
    pub(super) attempt_id: ToolExecutionAttemptId,
    pub(super) handler: BuiltinCoreHandlerV1,
    pub(super) resulting_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    pub(super) changed: Option<bool>,
}

impl BuiltinVerticalFixture {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary signed Built-in vertical root");
        let database_path = root.path().join("aura.db");
        let persistence = match PersistenceCoordinator::start(
            PersistenceOptions::new(
                database_path.clone(),
                root.path().join("artifacts"),
                aura_domain_common::RuntimeInstanceId::new(),
            )
            .with_tool_package_store_root(root.path().join("package-store")),
        )
        .expect("start signed Built-in persistence")
        {
            PersistenceStartup::Ready(value) => value,
            PersistenceStartup::RecoveryRequired(value) => {
                panic!("unexpected signed Built-in recovery: {}", value.reason)
            }
        };
        let materialized = persistence
            .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
            .expect("materialize signed Built-in release");
        assert_eq!(
            materialized.eligible_instance_ids.len(),
            6,
            "the five registry tools and the read-only activity query are signed built-ins"
        );
        assert!(materialized.quarantined_instance_ids.is_empty());
        persistence
            .publish_tool_projection_changes(&materialized.projection_changes)
            .expect("publish signed Built-in projections");

        let workspace_id = WorkspaceId::new();
        let session_id = SessionId::new();
        let turn_task_id = TurnTaskId::new();
        let agent_run_id = AgentRunId::new();
        seed_session_scope(
            &persistence,
            &database_path,
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            root.path(),
        )
        .expect("seed signed Built-in Session scope");
        let permission_snapshot = persistence
            .create_permission_snapshot(session_id, Some(turn_task_id), CorrelationId::new())
            .expect("create signed Built-in permission snapshot");
        let scope =
            ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, agent_run_id };
        let external_candidate = external_registry_candidate();
        let initial = ToolRegistryControlServiceV1
            .build_scope(&persistence, builtin_registry_request(&scope, &external_candidate))
            .expect("build signed Built-in Registry");
        assert!(initial.projection_publish_error.is_none());
        let initial_registry_snapshot_ref = initial.commit.registry_snapshot_ref;
        Self {
            _root: root,
            database_path,
            persistence,
            scope,
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            permission_snapshot,
            external_candidate,
            initial_registry_snapshot_ref,
        }
    }

    pub(super) fn external_definition_ref(
        &self,
        registry_snapshot_ref: &ToolRegistrySnapshotRef,
    ) -> ToolDefinitionSnapshotRef {
        ToolDefinitionSnapshotRef {
            tool_id: self.external_candidate.definition.tool_id,
            tool_version: self.external_candidate.definition.tool_version.clone(),
            registry_snapshot_ref: registry_snapshot_ref.clone(),
        }
    }

    pub(super) async fn execute(
        &self,
        registry_snapshot_ref: &ToolRegistrySnapshotRef,
        handler: BuiltinCoreHandlerV1,
        arguments: serde_json::Value,
    ) -> BuiltinExecutionObservationV1 {
        let corpus = self
            .persistence
            .read_tool_registry_search_corpus(registry_snapshot_ref)
            .expect("read exact signed Built-in Registry")
            .expect("signed Built-in Registry exists");
        let release = embedded_builtin_tool_definition_set_v1().expect("signed release");
        let release_entry = release
            .entries()
            .iter()
            .find(|entry| entry.execution_binding.builtin_handler() == handler)
            .expect("closed handler has one signed release entry");
        let definition_ref = ToolDefinitionSnapshotRef {
            tool_id: release_entry.definition.tool_id,
            tool_version: release_entry.definition.tool_version.clone(),
            registry_snapshot_ref: registry_snapshot_ref.clone(),
        };
        let schema = corpus
            .snapshot
            .schema_set
            .tools
            .iter()
            .find(|schema| {
                schema.tool_id == definition_ref.tool_id
                    && schema.tool_version == definition_ref.tool_version
            })
            .expect("signed Built-in is exposed in the exact schema set");
        let driver =
            P08ContractTestDriverV1::new(self.session_id, self.turn_task_id, self.agent_run_id);
        let batch = driver
            .batch_from_p05_provider_output(
                &corpus.snapshot,
                schema,
                "Exercise the exact signed Built-in handler",
                arguments,
            )
            .expect("normalize real P05 Provider ToolCall through P08 test driver");
        let call = batch.calls.first().expect("one normalized Built-in call").clone();
        assert_eq!(call.definition_ref.as_ref(), Some(&definition_ref));
        let binding = corpus
            .snapshot
            .effective_bindings
            .iter()
            .find(|binding| {
                binding.tool_id == definition_ref.tool_id
                    && binding.tool_version == definition_ref.tool_version
            })
            .expect("signed Built-in exact binding")
            .clone();
        let authority = self
            .persistence
            .load_exact_tool_execution_authority(&definition_ref, None)
            .expect("load signed Built-in execution authority")
            .expect("signed Built-in authority exists");
        let validated_call = aura_tool_scheduler::ValidatedToolCallV1 {
            call: call.clone(),
            definition: authority.definition.clone(),
            binding,
        };
        let mut runtime_required_scopes = vec![ResolvedScope::ToolRegistrySnapshot {
            registry_snapshot_ref: registry_snapshot_ref.clone(),
        }];
        let requires_registry_build =
            matches!(handler, BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload);
        if requires_registry_build {
            runtime_required_scopes.push(ResolvedScope::TurnTaskToolExposure {
                session_id: self.session_id,
                turn_task_id: self.turn_task_id,
                agent_run_id: self.agent_run_id,
            });
        }
        let prepared = ToolBatchAdmissionServiceV1
            .prepare(PrepareToolBatchExecutionInputV1 {
                batch,
                registry: &corpus.snapshot,
                definitions: vec![authority.definition.clone()],
                admission_authority: &AllowAuthority,
                settings: &ToolSettingsSnapshotV1::default(),
                planning_inputs: vec![FrozenToolCallPlanningInputV1 {
                    tool_call_id: validated_call.call.tool_call_id,
                    instance: authority.instance.clone(),
                    resources: ToolCallResourcePreflightV1::Resolved(
                        aura_tool_scheduler::ResolvedPackageResourceSetV1 {
                            scheduler_claims: Vec::new(),
                            permission_scopes: Vec::new(),
                            risk_flags: Vec::new(),
                            resolved_directories: Default::default(),
                        },
                    ),
                    runtime_required_scopes,
                    resolver_additional_scopes: Vec::new(),
                }],
                admitted_at: UtcTimestamp::now(),
                correlation_id: CorrelationId::new(),
                causation_id: None,
            })
            .expect("prepare signed Built-in batch");
        let ToolBatchPreparationOutcomeV1::Prepared(prepared) = prepared else {
            panic!("signed Built-in batch must be admitted")
        };
        let committed_batch =
            prepared.commit(&self.persistence).expect("commit signed Built-in batch admission");
        let operation_identity = InvocationIdentity {
            principal_id: aura_domain_common::PrincipalId::new(),
            actor: InvocationActor::ParentAgent,
            workspace_id: Some(self.workspace_id),
            session_id: Some(self.session_id),
            turn_task_id: Some(self.turn_task_id),
            agent_run_id: Some(self.agent_run_id),
            parent_agent_run_id: None,
            correlation_id: CorrelationId::new(),
            permission_snapshot_id: Some(self.permission_snapshot.id),
        };
        let evaluation = ToolBuiltinAssemblyServiceV1
            .evaluate(ToolBuiltinAssemblyInputV1 {
                admitted_batch: &committed_batch.admitted,
                planned_batch: &committed_batch.planned,
                planned_call: committed_batch
                    .planned
                    .runtime_calls
                    .first()
                    .expect("one planned signed Built-in call"),
                authority: &authority,
                operation_id: SecurityOperationId::new(),
                operation_identity: operation_identity.clone(),
                permission_snapshot: self.permission_snapshot.clone(),
                trust_state: &TrustState::SystemTrusted,
                permission_grants: &[],
                turn_denies: &[],
                evaluated_at: UtcTimestamp::now(),
            })
            .expect("evaluate signed Built-in through P04");
        assert!(matches!(evaluation.decision, PermissionDecision::Allow { .. }));
        let authorized_operation = evaluation
            .authorized_operation
            .expect("signed Built-in Full Access operation is authorized");
        let planned_call = committed_batch
            .planned
            .runtime_calls
            .first()
            .expect("one planned signed Built-in call");
        let scheduler = ToolExecutionSchedulerV1::new(4).expect("signed Built-in scheduler");
        let (_cancel_tx, mut cancellation) = watch::channel(false);
        let committed_attempt = ToolAttemptPreDispatchServiceV1
            .begin_builtin(
                &self.persistence,
                &scheduler,
                &mut cancellation,
                BeginAuthorizedToolBuiltinAttemptInputV1 {
                    admitted_batch: &committed_batch.admitted,
                    planned_batch: &committed_batch.planned,
                    planned_call,
                    authorized_operation,
                    evaluated_authority: &authority,
                    projection_state: committed_batch.projection_state.clone(),
                    admitted_event_id: committed_batch
                        .admitted_event_id()
                        .expect("signed Built-in admitted event"),
                    registry_build_request: requires_registry_build
                        .then(|| builtin_registry_request(&self.scope, &self.external_candidate)),
                    correlation_id: operation_identity.correlation_id,
                },
            )
            .await
            .expect("commit signed Built-in pre-dispatch boundary");
        let replay_binding = committed_attempt.binding.clone();
        let attempt_id = committed_attempt.attempt.tool_execution_attempt_id;
        let dispatched = committed_attempt
            .dispatch(
                &self.persistence,
                UtcTimestamp::now(),
                // R4: a real index for the fixture, so the model-executable query arm is
                // reachable here rather than being refused for want of a context.
                Some(crate::p07_builtin::BuiltinActivityObservationV1 {
                    index: aura_activity_runtime::ActivityIndex::default(),
                    runtime_instance_id: aura_domain_common::RuntimeInstanceId::new(),
                }),
            )
            .expect("dispatch closed signed Built-in handler");
        assert!(matches!(
            self.persistence.consume_authorized_builtin_core_handler_dispatch_v1(
                replay_binding,
                UtcTimestamp::now(),
            ),
            Err(PersistenceError::BuiltinCoreHandlerDispatchBindingUnavailable)
        ));
        let ToolRuntimeDispatchEvidenceV1::BuiltinCoreHandler(evidence) =
            &dispatched.dispatched.evidence
        else {
            panic!("signed Built-in dispatch returned another Runtime evidence kind")
        };
        assert_eq!(evidence.handler, handler);
        match &evidence.candidate {
            aura_tool_runtime::ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted { .. } => {}
            aura_tool_runtime::ToolRuntimeCandidateV1::DeterministicExternalFailure { error } => {
                panic!(
                    "signed Built-in handler {handler:?} returned deterministic error {}",
                    error.code
                )
            }
            candidate => {
                panic!("signed Built-in handler {handler:?} returned {candidate:?}")
            }
        }
        let resulting_registry_snapshot_ref = evidence.resulting_registry_snapshot_ref.clone();
        let changed = evidence.changed;
        let completed = ToolAttemptCompletionServiceV1
            .complete(
                &self.persistence,
                CompleteDispatchedToolAttemptInputV1 {
                    dispatched: dispatched.dispatched,
                    attempt_side_effect: dispatched.attempt_side_effect,
                    reconciliations: Vec::new(),
                    call_side_effect_evidence: dispatched.call_side_effect_evidence,
                    cancellation_intent: None,
                    completed_at: UtcTimestamp::now(),
                },
            )
            .expect("finalize signed Built-in Result");
        let CompleteDispatchedToolAttemptOutcomeV1::Finalized { result, .. } = completed else {
            panic!("signed Built-ins prohibit automatic retry")
        };
        assert!(matches!(result.outcome, aura_tool_contract::ToolResultOutcomeV1::Succeeded));
        BuiltinExecutionObservationV1 {
            result,
            attempt_id,
            handler,
            resulting_registry_snapshot_ref,
            changed,
        }
    }
}

pub(super) fn external_registry_candidate() -> RegistryCandidateV1 {
    let timestamp = UtcTimestamp::now();
    let tool_id = ToolId::new();
    let tool_version = ToolVersion::new(None, Revision::INITIAL).expect("external version");
    let source_contract_fingerprint = ToolSourceContractFingerprintV1::Mcp {
        normalized_mcp_tool_contract_fingerprint: NormalizedMcpToolContractFingerprintV1::new(
            CanonicalHashV1::sha256(b"P07 external Registry candidate"),
        ),
    };
    let definition = ToolDefinitionRecordV1::new(
        tool_id,
        tool_version.clone(),
        None,
        None,
        None,
        ToolCanonicalName::new("fixture.discoverable").expect("external name"),
        "Discoverable fixture".to_owned(),
        "A discoverable external fixture used only for Registry load tests.".to_owned(),
        "Discoverable external fixture".to_owned(),
        ToolInputSchemaV1::new(json!({
            "type": "object",
            "additionalProperties": false
        }))
        .expect("external input schema"),
        true,
        ToolResultContractV1::Unit,
        ToolRuntimeDescriptorV1::ExternalSource {
            source_contract_key: "fixture.external.mcp.v1".to_owned(),
        },
        Vec::new(),
        Vec::new(),
        ToolAudienceV1::SubagentConfigurable,
        ToolSourceKindV1::Mcp,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed: false,
            repeat_is_idempotent: true,
            may_create_persistent_effects: false,
        },
        CanonicalHashV1::sha256(b"P07 external Runtime closure"),
        Revision::INITIAL,
        timestamp,
    )
    .expect("external definition");
    let instance = ToolInstanceRecordV1 {
        tool_instance_id: ToolInstanceId::new(),
        tool_id,
        tool_version,
        source_kind: ToolSourceKindV1::Mcp,
        source_contract_fingerprint,
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
    RegistryCandidateV1 {
        definition,
        instance,
        source_evidence: ToolSourceEligibilityEvidenceV1::ExternalSource {
            source_enabled: true,
            source_trusted: true,
            source_runtime_healthy: true,
        },
        active_for_unpinned: true,
    }
}

pub(super) fn builtin_registry_request(
    scope: &ToolRegistryScopeV1,
    external_candidate: &RegistryCandidateV1,
) -> ToolRegistryBuildRequestV1 {
    let mut request = registry_request(scope.clone());
    request.non_package_candidates = vec![external_candidate.clone()];
    request
}
