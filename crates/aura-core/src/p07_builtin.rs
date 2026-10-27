#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "P08 production orchestration is deferred; the P07 Contract Driver exercises this assembly"
    )
)]

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ToolArtifactOwnerRef,
};
use aura_domain_common::ToolExecutionAttemptId;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, CorrelationId, ErrorCategory, ErrorEnvelope,
    SecurityOperationId, UtcTimestamp,
};
use aura_execution_contract::{
    AttemptSideEffectProofV1, SideEffectEvidenceProducerV1, VerifiedAttemptSideEffectEvidenceV1,
};
use aura_permission_domain::{
    NetworkAccessRequirement, OperationSecurityPlan, PermissionDecision, PermissionGrant,
    ResolvedScope, SecuritySource, TurnDenyRecord,
};
use aura_permission_engine::{DefaultPermissionEvaluator, PermissionEvaluation};
use aura_persistence::{
    ArtifactWriteRequest, ExactToolExecutionAuthorityV1, PersistenceCoordinator,
    ToolRegistryBuildRequestV1,
};
use aura_security_contract::TrustState;
use aura_security_contract::{
    InvocationIdentity, PermissionAllowAuthorizationBasisV1, PermissionSnapshot, ResolvedCapability,
};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, ToolCapabilitySummaryV1, ToolDescribeRequestV1,
    ToolDescribeResultV1, ToolListLoadedRequestV1, ToolListLoadedResultV1, ToolLoadRequestV1,
    ToolLoadStateV1, ToolLoadedEntryV1, ToolRegistryErrorCode, ToolRegistrySnapshotRef,
    ToolResourceClaimSummaryV1, ToolSafeIssueViewV1, ToolSchemaSnapshotV1, ToolSourceKindV1,
};
use aura_tool_registry::{
    BuiltinCoreHandlerDispatchBindingErrorV1, BuiltinCoreHandlerDispatchBindingV1,
    BuiltinCoreHandlerV1, ToolInstanceExecutionBindingV1,
};
use aura_tool_runtime::ToolRuntimeCandidateV1;
use aura_tool_scheduler::{
    AdmittedToolBatchV1, CallSideEffectEvidenceV1, PlannedToolBatchV1, PlannedToolRuntimeCallV1,
    security_source_for_admitted_tool_call_v1,
};
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    p07::{
        ToolSandboxAssemblyErrorV1, source_security_fingerprint, validate_common_authority_chain,
        validate_trust_binding,
    },
    p07_registry::{ToolRegistryControlErrorV1, ToolRegistryControlServiceV1},
};

const BUILTIN_CORE_HANDLER_OPERATION_VERSION_V1: u32 = 1;

pub(crate) struct ToolBuiltinAssemblyInputV1<'a> {
    pub admitted_batch: &'a AdmittedToolBatchV1,
    pub planned_batch: &'a PlannedToolBatchV1,
    pub planned_call: &'a PlannedToolRuntimeCallV1,
    pub authority: &'a ExactToolExecutionAuthorityV1,
    pub operation_id: SecurityOperationId,
    pub operation_identity: InvocationIdentity,
    pub permission_snapshot: PermissionSnapshot,
    pub trust_state: &'a TrustState,
    pub permission_grants: &'a [PermissionGrant],
    pub turn_denies: &'a [TurnDenyRecord],
    pub evaluated_at: UtcTimestamp,
}

pub(crate) struct ToolBuiltinAssemblyEvaluationV1 {
    pub decision: PermissionDecision,
    pub authorized_operation: Option<AuthorizedToolBuiltinOperationV1>,
}

pub(crate) struct AuthorizedToolBuiltinOperationV1 {
    operation_id: SecurityOperationId,
    operation_identity: InvocationIdentity,
    operation_fingerprint: ContentHash,
    permission_snapshot: PermissionSnapshot,
    authorization_basis: PermissionAllowAuthorizationBasisV1,
    tool_batch_id: aura_domain_common::ToolBatchId,
    tool_call_id: aura_domain_common::ToolCallId,
    execution_plan_hash: CanonicalHashV1,
    source: aura_permission_domain::ToolSecuritySourceRefV1,
    source_security_fingerprint: ContentHash,
    handler: BuiltinCoreHandlerV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct BuiltinCoreHandlerOperationPreimageV1<'a> {
    operation_version: u32,
    invocation_identity: &'a aura_tool_contract::CoreInvocationIdentityV1,
    tool_batch_id: aura_domain_common::ToolBatchId,
    tool_call_id: aura_domain_common::ToolCallId,
    source: &'a aura_permission_domain::ToolSecuritySourceRefV1,
    source_security_fingerprint: &'a ContentHash,
    execution_plan_hash: &'a CanonicalHashV1,
    handler: BuiltinCoreHandlerV1,
    capabilities: &'a [ResolvedCapability],
    scopes: &'a [ResolvedScope],
    purpose: &'a aura_tool_contract::ToolPurposeTextV1,
    arguments: &'a Value,
    arguments_hash: &'a aura_domain_common::jcs::CanonicalSha256HashV1,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ToolBuiltinAssemblyServiceV1;

impl ToolBuiltinAssemblyServiceV1 {
    pub(crate) fn evaluate(
        self,
        input: ToolBuiltinAssemblyInputV1<'_>,
    ) -> Result<ToolBuiltinAssemblyEvaluationV1, ToolBuiltinAssemblyErrorV1> {
        let plan_call = input
            .planned_batch
            .execution_plan
            .calls
            .iter()
            .find(|call| call.tool_call_id == input.planned_call.validated_call.call.tool_call_id)
            .ok_or(ToolBuiltinAssemblyErrorV1::AuthorityMismatch)?;
        validate_common_authority_chain(
            input.admitted_batch,
            input.planned_batch,
            input.planned_call,
            input.authority,
            &input.operation_identity,
            &input.permission_snapshot,
            plan_call.timeout_ms,
            false,
        )?;
        let binding = input
            .authority
            .builtin_execution_binding()
            .ok_or(ToolBuiltinAssemblyErrorV1::HandlerBindingMissing)?;
        let ToolInstanceExecutionBindingV1::BuiltInCoreHandler { handler } = binding;
        if input.authority.package.is_some()
            || input.authority.definition.source_kind != ToolSourceKindV1::BuiltIn
            || input.authority.instance.source_kind != ToolSourceKindV1::BuiltIn
            || handler.builtin_key() != input.authority.definition.canonical_name.as_str()
        {
            return Err(ToolBuiltinAssemblyErrorV1::AuthorityMismatch);
        }
        validate_builtin_capabilities_and_scopes(
            handler,
            input.admitted_batch,
            input.planned_call,
            &input.authority.definition.capabilities,
        )?;
        let source = security_source_for_admitted_tool_call_v1(
            &input.planned_call.validated_call,
            &input.authority.registry,
        )
        .map_err(|_| ToolBuiltinAssemblyErrorV1::AuthorityMismatch)?;
        let SecuritySource::Tool { tool } = &source else {
            return Err(ToolBuiltinAssemblyErrorV1::AuthorityMismatch);
        };
        let tool_source = (**tool).clone();
        let source_security_fingerprint = source_security_fingerprint(input.authority, tool)?;
        validate_trust_binding(tool.source_kind, &source_security_fingerprint, input.trust_state)?;
        if input.trust_state != &TrustState::SystemTrusted {
            return Err(ToolBuiltinAssemblyErrorV1::TrustAuthorityMismatch);
        }
        let NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash } =
            &input.planned_call.validated_call.call.input
        else {
            return Err(ToolBuiltinAssemblyErrorV1::InputNotReady);
        };
        let operation_fingerprint_hash = canonical_sha256(&BuiltinCoreHandlerOperationPreimageV1 {
            operation_version: BUILTIN_CORE_HANDLER_OPERATION_VERSION_V1,
            invocation_identity: &input.admitted_batch.batch.invocation_identity,
            tool_batch_id: input.admitted_batch.batch.tool_batch_id,
            tool_call_id: input.planned_call.validated_call.call.tool_call_id,
            source: tool,
            source_security_fingerprint: &source_security_fingerprint,
            execution_plan_hash: &input.planned_batch.execution_plan.execution_plan_hash,
            handler,
            capabilities: &input.authority.definition.capabilities,
            scopes: &input.planned_call.permission_scopes,
            purpose,
            arguments,
            arguments_hash,
        })
        .map_err(|_| ToolBuiltinAssemblyErrorV1::Canonicalization)?;
        let operation_fingerprint =
            ContentHash::new(ContentHashAlgorithm::Sha256, operation_fingerprint_hash.hex())
                .map_err(|_| ToolBuiltinAssemblyErrorV1::Canonicalization)?;
        let mut operation_identity = input.operation_identity;
        operation_identity.permission_snapshot_id = Some(input.permission_snapshot.id);
        let security_plan = OperationSecurityPlan {
            operation_id: input.operation_id,
            identity: operation_identity.clone(),
            source: source.clone(),
            capabilities: input.authority.definition.capabilities.clone(),
            scopes: input.planned_call.permission_scopes.clone(),
            risk_flags: input.planned_call.risk_flags.clone(),
            purpose: Some(purpose.as_str().to_owned()),
            source_fingerprint: Some(source_security_fingerprint.clone()),
            requested_sandbox: None,
            network_access: NetworkAccessRequirement::NotRequired,
            network_plan: None,
            operation_fingerprint: operation_fingerprint.clone(),
        };
        let evaluated =
            DefaultPermissionEvaluator.evaluate_with_authorization_basis(&PermissionEvaluation {
                plan: &security_plan,
                snapshot: input.permission_snapshot.clone(),
                source_enabled: input.authority.instance.enabled,
                trust_state: input.trust_state,
                grants: input.permission_grants,
                turn_denies: input.turn_denies,
                available_sandbox_modes: &[],
                network_control_available: true,
                allow_unrestricted_inline_scripts_in_full_access: false,
                now: input.evaluated_at,
            });
        let permission_snapshot = match &evaluated.decision {
            PermissionDecision::Allow { snapshot } => snapshot.clone(),
            _ => {
                return Ok(ToolBuiltinAssemblyEvaluationV1 {
                    decision: evaluated.decision,
                    authorized_operation: None,
                });
            }
        };
        let authorization_basis = evaluated
            .allow_authorization_basis
            .ok_or(ToolBuiltinAssemblyErrorV1::AuthorizationBasisMissing)?;
        Ok(ToolBuiltinAssemblyEvaluationV1 {
            decision: evaluated.decision,
            authorized_operation: Some(AuthorizedToolBuiltinOperationV1 {
                operation_id: input.operation_id,
                operation_identity,
                operation_fingerprint,
                permission_snapshot,
                authorization_basis,
                tool_batch_id: input.admitted_batch.batch.tool_batch_id,
                tool_call_id: input.planned_call.validated_call.call.tool_call_id,
                execution_plan_hash: input.planned_batch.execution_plan.execution_plan_hash.clone(),
                source: tool_source,
                source_security_fingerprint,
                handler,
            }),
        })
    }
}

impl AuthorizedToolBuiltinOperationV1 {
    pub(crate) fn materialize_dispatch(
        self,
        tool_execution_attempt_id: ToolExecutionAttemptId,
        prepared_at: UtcTimestamp,
    ) -> Result<BuiltinCoreHandlerDispatchBindingV1, ToolBuiltinAssemblyErrorV1> {
        BuiltinCoreHandlerDispatchBindingV1::new(
            self.operation_id,
            self.operation_identity,
            self.operation_fingerprint,
            self.permission_snapshot,
            self.authorization_basis,
            self.tool_batch_id,
            self.tool_call_id,
            tool_execution_attempt_id,
            self.execution_plan_hash,
            self.source,
            self.source_security_fingerprint,
            self.handler,
            prepared_at,
        )
        .map_err(ToolBuiltinAssemblyErrorV1::DispatchBinding)
    }
}

fn validate_builtin_capabilities_and_scopes(
    handler: BuiltinCoreHandlerV1,
    admitted_batch: &AdmittedToolBatchV1,
    planned_call: &PlannedToolRuntimeCallV1,
    capabilities: &[ResolvedCapability],
) -> Result<(), ToolBuiltinAssemblyErrorV1> {
    let expected_capabilities: &[ResolvedCapability] = match handler {
        BuiltinCoreHandlerV1::ToolsSearch
        | BuiltinCoreHandlerV1::ToolsDescribe
        | BuiltinCoreHandlerV1::ToolsListLoaded
        // R4: the run-information query is a **read-only observer**. It is grouped with the other
        // read-only builtins, and it deliberately does NOT gain `ToolEphemeralExposureWrite`: the
        // query changes no exposure, starts and stops nothing, and takes no effect on any resource.
        // Granting it the exposure-write capability would hand a model a mutation authority it has
        // no use for, which is exactly the kind of over-grant this match exists to prevent. It is
        // named explicitly rather than folded into a wildcard so that adding a handler later cannot
        // silently inherit read-only authority.
        | BuiltinCoreHandlerV1::ActivityQuery => &[ResolvedCapability::ToolRegistryRead],
        BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload => {
            &[ResolvedCapability::ToolRegistryRead, ResolvedCapability::ToolEphemeralExposureWrite]
        }
    };
    if capabilities != expected_capabilities || !planned_call.risk_flags.is_empty() {
        return Err(ToolBuiltinAssemblyErrorV1::AuthorityMismatch);
    }
    let registry_scope = ResolvedScope::ToolRegistrySnapshot {
        registry_snapshot_ref: admitted_batch.batch.registry_snapshot_ref.clone(),
    };
    let mut expected_scopes = vec![registry_scope];
    if matches!(handler, BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload) {
        let invocation = &admitted_batch.batch.invocation_identity;
        expected_scopes.push(ResolvedScope::TurnTaskToolExposure {
            session_id: invocation.session_id,
            turn_task_id: invocation.turn_task_id,
            agent_run_id: invocation.agent_run_id,
        });
    }
    if planned_call.permission_scopes.len() != expected_scopes.len()
        || expected_scopes.iter().any(|scope| !planned_call.permission_scopes.contains(scope))
    {
        return Err(ToolBuiltinAssemblyErrorV1::AuthorityMismatch);
    }
    Ok(())
}

pub(crate) struct BuiltinCoreHandlerDispatchEvidenceV1 {
    pub handler: BuiltinCoreHandlerV1,
    pub binding_hash: CanonicalHashV1,
    pub source_registry_snapshot_ref: ToolRegistrySnapshotRef,
    pub resulting_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    pub changed: Option<bool>,
    pub projection_publish_failed: bool,
    pub candidate: ToolRuntimeCandidateV1,
}

pub(crate) struct BuiltinCoreHandlerInvocationOutcomeV1 {
    pub evidence: BuiltinCoreHandlerDispatchEvidenceV1,
    pub attempt_side_effect: VerifiedAttemptSideEffectEvidenceV1,
    pub call_side_effect_evidence: CallSideEffectEvidenceV1,
}

#[expect(
    clippy::too_many_arguments,
    reason = "composition passes separately verified authority, registry and observation facts"
)]
pub(crate) fn invoke_builtin_core_handler_v1(
    persistence: &PersistenceCoordinator,
    binding: &BuiltinCoreHandlerDispatchBindingV1,
    arguments: &Value,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    registry_build_request: Option<ToolRegistryBuildRequestV1>,
    correlation_id: CorrelationId,
    invoked_at: UtcTimestamp,
    // R4: the process's activity index, when the caller has one. It is `Option` so a caller outside
    // the assembled runtime (a test rig or a conformance driver) simply has no observation context,
    // and the query handler refuses rather than inventing an answer.
    observation: Option<&BuiltinActivityObservationV1>,
) -> Result<BuiltinCoreHandlerInvocationOutcomeV1, ToolBuiltinHandlerErrorV1> {
    if binding.source.definition_ref.registry_snapshot_ref != registry.snapshot_ref {
        return Err(ToolBuiltinHandlerErrorV1::AuthorityMismatch);
    }
    let invocation = match invoke_handler_inner(
        persistence,
        binding.handler,
        arguments,
        registry,
        registry_build_request,
        correlation_id,
        // R4: the observation context, when the caller assembled one. The binding supplies the
        // caller's real identity, which is what the query projects under; nothing about who is
        // asking is read from the model's arguments.
        observation,
        &binding.operation_identity,
    ) {
        Ok(invocation) => invocation,
        Err(error) => BuiltinHandlerInvocationV1 {
            candidate: ToolRuntimeCandidateV1::DeterministicExternalFailure {
                error: registry_handler_error(&error, correlation_id),
            },
            resulting_registry_snapshot_ref: None,
            changed: None,
            projection_publish_failed: false,
            effect: if matches!(
                binding.handler,
                BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload
            ) {
                BuiltinHandlerEffectV1::Unknown
            } else {
                BuiltinHandlerEffectV1::NoEffect
            },
        },
    };
    let effect_scope_hash = canonical_sha256(&json!({
        "binding_hash": binding.binding_hash,
        "handler": binding.handler,
        "source_registry_snapshot_ref": registry.snapshot_ref,
        "resulting_registry_snapshot_ref": invocation.resulting_registry_snapshot_ref,
    }))
    .map_err(|_| ToolBuiltinHandlerErrorV1::Canonicalization)?;
    let (proof, call_side_effect_evidence) = match invocation.effect {
        BuiltinHandlerEffectV1::NoEffect => {
            let evidence_ref = write_effect_evidence(
                persistence,
                binding.tool_execution_attempt_id,
                "application/vnd.aura.builtin-no-effect-evidence.v1+json",
                &json!({
                    "evidence_version": 1,
                    "handler": binding.handler,
                    "binding_hash": binding.binding_hash,
                    "source_registry_snapshot_ref": registry.snapshot_ref,
                    "invoked_at": invoked_at,
                }),
            )?;
            (
                AttemptSideEffectProofV1::NoEffect { evidence_ref },
                CallSideEffectEvidenceV1::default(),
            )
        }
        BuiltinHandlerEffectV1::FullyApplied => {
            let receipt_ref = write_effect_evidence(
                persistence,
                binding.tool_execution_attempt_id,
                "application/vnd.aura.builtin-transaction-receipt.v1+json",
                &json!({
                    "evidence_version": 1,
                    "handler": binding.handler,
                    "binding_hash": binding.binding_hash,
                    "source_registry_snapshot_ref": registry.snapshot_ref,
                    "resulting_registry_snapshot_ref": invocation.resulting_registry_snapshot_ref,
                    "invoked_at": invoked_at,
                }),
            )?;
            let postcondition_ref = write_effect_evidence(
                persistence,
                binding.tool_execution_attempt_id,
                "application/vnd.aura.builtin-postcondition.v1+json",
                &json!({
                    "evidence_version": 1,
                    "handler": binding.handler,
                    "resulting_registry_snapshot_ref": invocation.resulting_registry_snapshot_ref,
                    "changed": invocation.changed,
                    "invoked_at": invoked_at,
                }),
            )?;
            (
                AttemptSideEffectProofV1::FullyApplied { receipt_ref, postcondition_ref },
                CallSideEffectEvidenceV1 {
                    final_postcondition_verified: true,
                    all_effects_covered_by_final_state: true,
                    baseline_restoration_verified: false,
                    duplicate_effects_absent_or_deduplicated: true,
                    aggregate_evidence_ref: None,
                },
            )
        }
        BuiltinHandlerEffectV1::Unknown => {
            let evidence_ref = write_effect_evidence(
                persistence,
                binding.tool_execution_attempt_id,
                "application/vnd.aura.builtin-unknown-effect-evidence.v1+json",
                &json!({
                    "evidence_version": 1,
                    "handler": binding.handler,
                    "binding_hash": binding.binding_hash,
                    "invoked_at": invoked_at,
                }),
            )?;
            (
                AttemptSideEffectProofV1::Unknown { evidence_ref },
                CallSideEffectEvidenceV1::default(),
            )
        }
    };
    let attempt_side_effect = VerifiedAttemptSideEffectEvidenceV1::from_core_evidence(
        binding.tool_execution_attempt_id,
        effect_scope_hash,
        SideEffectEvidenceProducerV1::CoreTransaction,
        proof,
    )
    .map_err(|_| ToolBuiltinHandlerErrorV1::Canonicalization)?;
    Ok(BuiltinCoreHandlerInvocationOutcomeV1 {
        evidence: BuiltinCoreHandlerDispatchEvidenceV1 {
            handler: binding.handler,
            binding_hash: binding.binding_hash.clone(),
            source_registry_snapshot_ref: registry.snapshot_ref.clone(),
            resulting_registry_snapshot_ref: invocation.resulting_registry_snapshot_ref,
            changed: invocation.changed,
            projection_publish_failed: invocation.projection_publish_failed,
            candidate: invocation.candidate,
        },
        attempt_side_effect,
        call_side_effect_evidence,
    })
}

struct BuiltinHandlerInvocationV1 {
    candidate: ToolRuntimeCandidateV1,
    resulting_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    changed: Option<bool>,
    projection_publish_failed: bool,
    effect: BuiltinHandlerEffectV1,
}

enum BuiltinHandlerEffectV1 {
    NoEffect,
    FullyApplied,
    Unknown,
}

/// R4: everything the run-information query handler needs, handed in explicitly.
///
/// WHY THIS IS A PARAMETER AND NOT A FIELD ON THE SERVICE
/// `p07_builtin` is a registry/persistence layer. It holds no `RuntimeServices`, and giving it one
/// would couple registry control to the whole runtime. The query needs exactly two things - the
/// process's activity index and the runtime instance that identifies this generation - so they are
/// passed as a value, and a caller that has neither passes `None` and gets a closed refusal rather
/// than a fabricated empty answer.
#[derive(Clone)]
pub(crate) struct BuiltinActivityObservationV1 {
    pub index: aura_activity_runtime::ActivityIndex,
    pub runtime_instance_id: aura_domain_common::RuntimeInstanceId,
}

#[expect(
    clippy::too_many_arguments,
    reason = "composition keeps request data separate from the verified invocation identity"
)]
fn invoke_handler_inner(
    persistence: &PersistenceCoordinator,
    handler: BuiltinCoreHandlerV1,
    arguments: &Value,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    registry_build_request: Option<ToolRegistryBuildRequestV1>,
    correlation_id: CorrelationId,
    // R4: the observation context, and the caller's **real** identity.
    //
    // `invocation_identity` is the binding's own operation identity - the same session, workspace
    // and Agent run this call was admitted under. It is the only legitimate source of "who is
    // asking", and it is deliberately not taken from the model's arguments: a request that carried
    // its own session id would be able to widen its own visibility by naming one.
    observation: Option<&BuiltinActivityObservationV1>,
    invocation_identity: &aura_security_contract::InvocationIdentity,
) -> Result<BuiltinHandlerInvocationV1, ToolRegistryControlErrorV1> {
    let service = ToolRegistryControlServiceV1;
    let response = match handler {
        BuiltinCoreHandlerV1::ToolsSearch => {
            let request: aura_tool_contract::ToolSearchRequestV1 =
                serde_json::from_value(arguments.clone())
                    .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            let visibility = canonical_sha256(&json!({
                "visibility_version": 1,
                "scope": registry.scope,
                "registry_snapshot_ref": registry.snapshot_ref,
            }))
            .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            serde_json::to_value(service.search(
                persistence,
                &registry.snapshot_ref,
                &visibility,
                &request,
            )?)
            .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?
        }
        BuiltinCoreHandlerV1::ToolsDescribe => {
            let request: ToolDescribeRequestV1 = serde_json::from_value(arguments.clone())
                .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            if request.definition_ref.registry_snapshot_ref != registry.snapshot_ref {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            let described = service.describe(persistence, &request.definition_ref)?;
            serde_json::to_value(describe_result(persistence, registry, request, described)?)
                .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?
        }
        BuiltinCoreHandlerV1::ToolsListLoaded => {
            let request: ToolListLoadedRequestV1 = serde_json::from_value(arguments.clone())
                .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            if request.registry_snapshot_ref != registry.snapshot_ref {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            let exact = service.list_loaded(persistence, &request.registry_snapshot_ref)?;
            let tools = registry
                .catalog_entries
                .iter()
                .filter(|entry| {
                    exact.iter().any(|definition| {
                        definition.tool_id == entry.tool_id
                            && definition.tool_version == entry.tool_version
                    }) && entry.load_state != ToolLoadStateV1::Discoverable
                })
                .map(|entry| {
                    let definition_ref = aura_tool_contract::ToolDefinitionSnapshotRef {
                        tool_id: entry.tool_id,
                        tool_version: entry.tool_version.clone(),
                        registry_snapshot_ref: registry.snapshot_ref.clone(),
                    };
                    let audience = persistence
                        .describe_tool_definition_exact(&definition_ref)?
                        .ok_or(ToolRegistryControlErrorV1::DefinitionUnavailable)?
                        .definition
                        .audience;
                    Ok(ToolLoadedEntryV1 {
                        definition_ref,
                        tool_instance_id: entry.tool_instance_id,
                        canonical_name: entry.canonical_name.clone(),
                        display_name: entry.display_name.clone(),
                        model_summary: entry.summary.clone(),
                        source: entry.source,
                        audience,
                        load_state: entry.load_state,
                    })
                })
                .collect::<Result<Vec<_>, ToolRegistryControlErrorV1>>()?;
            serde_json::to_value(ToolListLoadedResultV1 {
                registry_snapshot_ref: registry.snapshot_ref.clone(),
                tools,
            })
            .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?
        }
        BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload => {
            let request: ToolLoadRequestV1 = serde_json::from_value(arguments.clone())
                .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            if request
                .definitions
                .iter()
                .any(|definition| definition.registry_snapshot_ref != registry.snapshot_ref)
            {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            let mut registry_build_request =
                registry_build_request.ok_or(ToolRegistryControlErrorV1::BuildContextMissing)?;
            if registry_build_request.scope != registry.scope {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            registry_build_request.created_at = UtcTimestamp::now();
            registry_build_request.correlation_id = correlation_id;
            let published = if handler == BuiltinCoreHandlerV1::ToolsLoad {
                service.load(persistence, request, registry_build_request)?
            } else {
                service.unload(persistence, request, registry_build_request)?
            };
            let response = published.result;
            return Ok(BuiltinHandlerInvocationV1 {
                candidate: ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted {
                    response_body: aura_domain_common::jcs::canonical_json_jcs(&response)
                        .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
                },
                resulting_registry_snapshot_ref: Some(
                    response.resulting_registry_snapshot_ref.clone(),
                ),
                changed: Some(response.changed),
                projection_publish_failed: published.projection_publish_error.is_some(),
                effect: if response.changed {
                    BuiltinHandlerEffectV1::FullyApplied
                } else {
                    BuiltinHandlerEffectV1::NoEffect
                },
            });
        }
        // R4: THE MODEL-EXECUTABLE RUN-INFORMATION QUERY.
        //
        // This is the execution side of the entry the summary names. It is a **read-only
        // observer**: it takes no resource claim, publishes nothing into the index, starts and
        // stops nothing, and its result cannot change any tool's fate. The page it returns is the
        // same fixed-snapshot page the IPC route returns, re-projected under the **caller's own**
        // real visibility - so a model cannot widen what it may see by asking, and another
        // session's private paths appear only when the caller already reached the same physical
        // target.
        BuiltinCoreHandlerV1::ActivityQuery => {
            let request: aura_activity_domain::AgentActivityQueryInput =
                serde_json::from_value(arguments.clone())
                    .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            // A caller with no observation context is refused rather than told "nothing is
            // running": the two are different answers and only one of them is true.
            let observation = observation.ok_or(ToolRegistryControlErrorV1::BuildContextMissing)?;
            let query =
                request.to_query().map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            // The caller's real position, from the binding's own operation identity. It is never
            // taken from the request, so naming a session cannot widen visibility.
            let session_id =
                invocation_identity.session_id.ok_or(ToolRegistryControlErrorV1::RequestInvalid)?;
            let workspace_id = invocation_identity
                .workspace_id
                .ok_or(ToolRegistryControlErrorV1::RequestInvalid)?;
            let visibility = aura_activity_runtime::ActivityVisibility::for_session(
                observation.runtime_instance_id,
                session_id,
                workspace_id,
            );
            let page = crate::agent_activity::query::page(
                &observation.index,
                visibility,
                query,
                request.expected_revision,
                request.offset,
                request.effective_limit(),
                true,
            )
            .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            let response = aura_activity_domain::AgentActivityQueryResultV1 {
                contract_version: aura_activity_domain::ACTIVITY_QUERY_CONTRACT_VERSION,
                page,
            };
            return Ok(BuiltinHandlerInvocationV1 {
                candidate: ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted {
                    response_body: aura_domain_common::jcs::canonical_json_jcs(&response)
                        .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
                },
                // A query publishes no new registry snapshot and changes no exposure.
                resulting_registry_snapshot_ref: None,
                changed: None,
                projection_publish_failed: false,
                // Asking a question has no effect on the world, and saying so is what keeps the
                // attempt's side-effect evidence honest.
                effect: BuiltinHandlerEffectV1::NoEffect,
            });
        }
    };
    Ok(BuiltinHandlerInvocationV1 {
        candidate: ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted {
            response_body: aura_domain_common::jcs::canonical_json_jcs(&response)
                .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
        },
        resulting_registry_snapshot_ref: None,
        changed: None,
        projection_publish_failed: false,
        effect: BuiltinHandlerEffectV1::NoEffect,
    })
}

/// Service entry; Core has already bound the exact stored Snapshot and
/// derived the build context. P07's legacy Attempt adapter exits in R08.
///
/// R4: this entry serves the Renderer/IPC tool-administration surface, which has no model step and
/// therefore no observation context and no Agent invocation identity. It passes `None` and a
/// user-shaped identity, so the one handler that needs an observation context
/// (`agent.activity.query`) is **refused** here rather than being answered from a fabricated empty
/// index. That refusal is the correct behaviour: this surface was never the query's caller.
pub(crate) fn invoke_broker_service(
    persistence: &PersistenceCoordinator,
    handler: BuiltinCoreHandlerV1,
    arguments: &Value,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    build: Option<ToolRegistryBuildRequestV1>,
    correlation: CorrelationId,
) -> Result<aura_tool_contract::ToolBrokerInvokeResult, ToolRegistryControlErrorV1> {
    // The administration surface's own identity: it acts as the runtime user, with no Agent run.
    // It is built here so the query handler cannot mistake an administrative call for a model step.
    let administration_identity = aura_security_contract::InvocationIdentity {
        principal_id: aura_domain_common::PrincipalId::new(),
        actor: aura_security_contract::InvocationActor::User,
        workspace_id: None,
        session_id: None,
        turn_task_id: None,
        agent_run_id: None,
        parent_agent_run_id: None,
        correlation_id: correlation,
        permission_snapshot_id: None,
    };
    let result = invoke_handler_inner(
        persistence,
        handler,
        arguments,
        registry,
        build,
        correlation,
        None,
        &administration_identity,
    )?;
    let ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted { response_body } = result.candidate
    else {
        return Err(ToolRegistryControlErrorV1::ResponseInvalid);
    };
    Ok(aura_tool_contract::ToolBrokerInvokeResult {
        value: serde_json::from_slice(&response_body)
            .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
        resulting_registry_snapshot_ref: result.resulting_registry_snapshot_ref,
        changed: result.changed,
    })
}

fn describe_result(
    persistence: &PersistenceCoordinator,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    request: ToolDescribeRequestV1,
    described: aura_persistence::ExactToolDefinitionDescriptionV1,
) -> Result<ToolDescribeResultV1, ToolRegistryControlErrorV1> {
    let entry = registry
        .catalog_entries
        .iter()
        .find(|entry| {
            entry.tool_id == request.definition_ref.tool_id
                && entry.tool_version == request.definition_ref.tool_version
        })
        .ok_or(ToolRegistryControlErrorV1::DefinitionUnavailable)?;
    let schema = ToolSchemaSnapshotV1::new(
        described.definition.tool_id,
        described.definition.tool_version.clone(),
        described.definition.canonical_name.clone(),
        described.definition.model_description.clone(),
        described.definition.input_schema.clone(),
        described.definition.strict,
    )
    .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?;
    let capability_summary = described
        .definition
        .capabilities
        .iter()
        .map(|capability| {
            let capability = serde_json::to_value(capability)
                .ok()
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
                .ok_or(ToolRegistryControlErrorV1::ResponseInvalid)?;
            Ok(ToolCapabilitySummaryV1 { summary: capability.replace('_', " "), capability })
        })
        .collect::<Result<Vec<_>, ToolRegistryControlErrorV1>>()?;
    let resource_claim_summary = if described.definition.tool_package_id.is_some() {
        persistence
            .load_exact_tool_execution_authority(&request.definition_ref, None)?
            .and_then(|authority| authority.package)
            .map(|package| {
                package
                    .manifest_tool()
                    .resource_claims
                    .iter()
                    .map(package_resource_claim_summary)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default()
    } else {
        // Built-ins and typed External Sources are not Package authority and
        // must never be reinterpreted as one merely to build a description.
        Vec::new()
    };
    let healthy = described.instance.enabled
        && described.instance.runtime_health == aura_tool_domain::ToolRuntimeHealthV1::Healthy;
    Ok(ToolDescribeResultV1 {
        definition_ref: request.definition_ref,
        canonical_name: described.definition.canonical_name,
        display_name: described.definition.display_name,
        model_summary: described.definition.model_summary,
        model_description: described.definition.model_description,
        schema,
        result_contract: described.definition.result_contract,
        source: described.definition.source_kind,
        audience: described.definition.audience,
        capability_summary,
        resource_claim_summary,
        load_state: entry.load_state,
        currently_loadable: healthy,
        safe_unavailability_reason: (!healthy).then(|| ToolSafeIssueViewV1 {
            code: ToolRegistryErrorCode::Disabled.error_code(),
            category: ErrorCategory::Conflict,
            message: "The exact tool instance is not currently eligible.".to_owned(),
            retryable: false,
        }),
    })
}

fn package_resource_claim_summary(
    claim: &aura_tool_contract::PackageDeclaredResourceClaimV1,
) -> Result<ToolResourceClaimSummaryV1, ToolRegistryControlErrorV1> {
    let value =
        serde_json::to_value(claim).map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?;
    let resource_kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ToolRegistryControlErrorV1::ResponseInvalid)?
        .to_owned();
    let access = value
        .get("access")
        .or_else(|| value.get("selector").and_then(|selector| selector.get("access")))
        .map(|value| {
            value
                .as_str()
                .map(|value| vec![value.to_owned()])
                .or_else(|| {
                    value.as_array().map(|values| {
                        values.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect()
                    })
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    Ok(ToolResourceClaimSummaryV1 { claim_key: claim.claim_key().clone(), resource_kind, access })
}

fn write_effect_evidence(
    persistence: &PersistenceCoordinator,
    attempt_id: ToolExecutionAttemptId,
    media_type: &str,
    value: &Value,
) -> Result<aura_domain_common::ArtifactId, ToolBuiltinHandlerErrorV1> {
    Ok(persistence
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::ToolExecution,
            media_type: media_type.to_owned(),
            access: ArtifactAccess::NONE,
            owner: ArtifactOwnerRef::Tool {
                owner: ToolArtifactOwnerRef::ExecutionAttempt {
                    tool_execution_attempt_id: attempt_id,
                },
            },
            bytes: aura_domain_common::jcs::canonical_json_jcs(value)
                .map_err(|_| ToolBuiltinHandlerErrorV1::Canonicalization)?,
        })?
        .artifact_id)
}

fn registry_handler_error(
    error: &ToolRegistryControlErrorV1,
    correlation_id: CorrelationId,
) -> ErrorEnvelope {
    let (code, category, message) = match error {
        ToolRegistryControlErrorV1::Search(error) => (
            error.public_code(),
            ErrorCategory::Conflict,
            "The exact Tool Registry search could not be completed.",
        ),
        ToolRegistryControlErrorV1::SnapshotUnavailable
        | ToolRegistryControlErrorV1::DefinitionUnavailable => (
            ToolRegistryErrorCode::SnapshotUnavailable,
            ErrorCategory::Unavailable,
            "The exact Tool Registry snapshot or definition is unavailable.",
        ),
        ToolRegistryControlErrorV1::RequestInvalid => (
            ToolRegistryErrorCode::SearchRequestInvalid,
            ErrorCategory::Validation,
            "The Built-in Tool request is invalid.",
        ),
        ToolRegistryControlErrorV1::SettingsInvalid
        | ToolRegistryControlErrorV1::ProjectionMissing
        | ToolRegistryControlErrorV1::ProjectionMultiplicity
        | ToolRegistryControlErrorV1::ResponseInvalid
        | ToolRegistryControlErrorV1::BuildContextMissing
        | ToolRegistryControlErrorV1::SnapshotScopeMismatch
        | ToolRegistryControlErrorV1::CleanupScopeInvalid
        | ToolRegistryControlErrorV1::EphemeralAuthorityMismatch
        | ToolRegistryControlErrorV1::SnapshotAdvancedDuringCleanup
        | ToolRegistryControlErrorV1::Persistence(_) => (
            ToolRegistryErrorCode::SnapshotCorrupted,
            ErrorCategory::Internal,
            "The Built-in Tool authority could not be completed safely.",
        ),
    };
    ErrorEnvelope::new(code.error_code(), category, message, false, correlation_id, None)
}

#[derive(Debug, Error)]
pub(crate) enum ToolBuiltinAssemblyErrorV1 {
    #[error("the signed Built-in execution authority does not match the admitted call")]
    AuthorityMismatch,
    #[error("the signed Built-in handler binding is missing")]
    HandlerBindingMissing,
    #[error("the Built-in operation requires system trust authority")]
    TrustAuthorityMismatch,
    #[error("the admitted Built-in input is not ready")]
    InputNotReady,
    #[error("the P04 Allow result did not include an authorization basis")]
    AuthorizationBasisMissing,
    #[error("the Built-in operation fingerprint could not be canonicalized")]
    Canonicalization,
    #[error(transparent)]
    CommonAssembly(#[from] ToolSandboxAssemblyErrorV1),
    #[error(transparent)]
    DispatchBinding(#[from] BuiltinCoreHandlerDispatchBindingErrorV1),
}

#[derive(Debug, Error)]
pub(crate) enum ToolBuiltinHandlerErrorV1 {
    #[error("the Built-in handler does not match the exact Registry authority")]
    AuthorityMismatch,
    #[error("the Built-in handler evidence could not be canonicalized")]
    Canonicalization,
    #[error(transparent)]
    Persistence(#[from] aura_persistence::PersistenceError),
}
