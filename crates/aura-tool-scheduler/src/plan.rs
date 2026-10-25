use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::Revision;
use aura_domain_common::ToolCallId;
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{
    ToolExecutionPlanCallV1, ToolExecutionPlanV1, ToolNotExecutedReasonV1, ToolRegistrySnapshotV1,
    ToolResultOutcomeV1, ToolResultV1, ToolSettingsSnapshotV1,
};
use aura_tool_domain::{ToolInstanceOverridesV1, ToolInstanceRecordV1};
use serde::Serialize;
use thiserror::Error;

use crate::{
    AdmittedToolBatchV1, PackageResourcePreflightRejectionV1, ResolvedPackageResourceSetV1,
    ToolInstanceExecutionGenerationKeyV1, ToolPreflightRejectionV1, ValidatedToolCallV1,
    compose_tool_operation_scopes_v1,
};

#[derive(Clone, Debug)]
pub enum ToolCallResourcePreflightV1 {
    Resolved(ResolvedPackageResourceSetV1),
    Rejected(PackageResourcePreflightRejectionV1),
}

#[derive(Clone, Debug)]
pub struct FrozenToolCallPlanningInputV1 {
    pub tool_call_id: ToolCallId,
    pub instance: ToolInstanceRecordV1,
    pub resources: ToolCallResourcePreflightV1,
    /// Scopes derived from the exact prepared Runtime operation. Package
    /// process launches use the verified Package Runtime authority; HTTP uses
    /// the same prepared Origin that produced the exact request URL.
    pub runtime_required_scopes: Vec<aura_permission_domain::ResolvedScope>,
    /// Additional exact scopes emitted by an authoritative resolver, such as
    /// private-network classification. These must not widen or replace a
    /// declared Claim scope.
    pub resolver_additional_scopes: Vec<aura_permission_domain::ResolvedScope>,
}

#[derive(Clone, Debug)]
pub struct PlannedToolRuntimeCallV1 {
    pub validated_call: ValidatedToolCallV1,
    pub generation: ToolInstanceExecutionGenerationKeyV1,
    pub permission_scopes: Vec<aura_permission_domain::ResolvedScope>,
    pub risk_flags: Vec<aura_security_contract::OperationRiskFlag>,
    pub resolved_directories:
        BTreeMap<aura_tool_contract::PackageResourceClaimKeyV1, std::path::PathBuf>,
}

#[derive(Clone, Debug)]
pub struct PlannedToolBatchV1 {
    pub execution_plan: ToolExecutionPlanV1,
    pub runtime_calls: Vec<PlannedToolRuntimeCallV1>,
    pub not_executed: Vec<ToolPreflightRejectionV1>,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolExecutionPlanningErrorV1 {
    #[error("the admitted batch, Registry Snapshot, and planning inputs are inconsistent")]
    AuthorityMismatch,
    #[error("the frozen Tool settings are invalid")]
    SettingsInvalid,
    #[error("the derived execution plan is invalid")]
    PlanInvalid,
}

/// Builds the immutable post-admission execution plan only after resource
/// preflight for every otherwise-valid call has completed. It never calls
/// P04, acquires a lock, creates a `tea_`, or dispatches a Runtime.
pub fn build_tool_execution_plan_v1(
    admitted: &AdmittedToolBatchV1,
    registry: &ToolRegistrySnapshotV1,
    settings: &ToolSettingsSnapshotV1,
    inputs: Vec<FrozenToolCallPlanningInputV1>,
) -> Result<PlannedToolBatchV1, ToolExecutionPlanningErrorV1> {
    settings.validate().map_err(|_| ToolExecutionPlanningErrorV1::SettingsInvalid)?;
    registry.validate().map_err(|_| ToolExecutionPlanningErrorV1::AuthorityMismatch)?;
    if registry.snapshot_ref != admitted.batch.registry_snapshot_ref {
        return Err(ToolExecutionPlanningErrorV1::AuthorityMismatch);
    }

    let mut input_by_id = BTreeMap::new();
    for input in inputs {
        if input_by_id.insert(input.tool_call_id, input).is_some() {
            return Err(ToolExecutionPlanningErrorV1::AuthorityMismatch);
        }
    }
    let validated_by_id = admitted
        .validated_calls
        .iter()
        .map(|call| (call.call.tool_call_id, call))
        .collect::<BTreeMap<_, _>>();
    if input_by_id.len() != validated_by_id.len()
        || input_by_id.keys().any(|id| !validated_by_id.contains_key(id))
    {
        return Err(ToolExecutionPlanningErrorV1::AuthorityMismatch);
    }

    let names = registry
        .catalog_entries
        .iter()
        .map(|entry| (entry.tool_id, entry.canonical_name.as_str()))
        .collect::<BTreeMap<_, _>>();
    let call_names = admitted
        .batch
        .calls
        .iter()
        .filter_map(|call| {
            // A Provider name absent from the frozen request has no Tool to plan; it is already
            // ToolNotFound. A resolved call must still name a Tool in this Registry Snapshot.
            let reference = call.definition_ref.as_ref()?;
            Some(
                names
                    .get(&reference.tool_id)
                    .copied()
                    .map(|name| (call.tool_call_id, name))
                    .ok_or(ToolExecutionPlanningErrorV1::AuthorityMismatch),
            )
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let named_calls = admitted
        .batch
        .calls
        .iter()
        .map(|call| {
            (call.tool_call_id, call_names.get(&call.tool_call_id).copied().unwrap_or_default())
        })
        .collect::<Vec<_>>();
    let dependencies = derive_control_dependencies(&named_calls);
    let atomic_groups = derive_execution_atomic_groups(&admitted.batch.calls, &call_names);
    let mut rejected = admitted
        .not_executed
        .iter()
        .cloned()
        .map(|rejection| (rejection.tool_call_id, rejection))
        .collect::<BTreeMap<_, _>>();
    let mut resolved = BTreeMap::new();

    for call in &admitted.validated_calls {
        let input = input_by_id
            .remove(&call.call.tool_call_id)
            .ok_or(ToolExecutionPlanningErrorV1::AuthorityMismatch)?;
        validate_instance_for_call(call, &input.instance)?;
        match input.resources {
            ToolCallResourcePreflightV1::Resolved(resources) => {
                resolved.insert(
                    call.call.tool_call_id,
                    (
                        input.instance,
                        resources,
                        input.runtime_required_scopes,
                        input.resolver_additional_scopes,
                    ),
                );
            }
            ToolCallResourcePreflightV1::Rejected(resource) => {
                rejected.insert(
                    call.call.tool_call_id,
                    ToolPreflightRejectionV1 {
                        tool_call_id: call.call.tool_call_id,
                        ordinal: call.call.ordinal,
                        reason: resource.reason,
                        error_code: Some(resource.error_code),
                        schema_validation: None,
                        input_issue: None,
                        input_diagnostic: None,
                    },
                );
            }
        }
    }

    propagate_atomic_group_rejections(&atomic_groups, &admitted.batch.calls, &mut rejected);
    propagate_dependency_rejections(&dependencies, &admitted.batch.calls, &mut rejected);

    let mut plan_calls = Vec::new();
    let mut runtime_calls = Vec::new();
    for call in &admitted.validated_calls {
        if rejected.contains_key(&call.call.tool_call_id) {
            continue;
        }
        let (instance, resources, runtime_required_scopes, resolver_additional_scopes) = resolved
            .remove(&call.call.tool_call_id)
            .ok_or(ToolExecutionPlanningErrorV1::AuthorityMismatch)?;
        let policy = resolve_execution_policy(settings, &instance.overrides)?;
        let execution_fingerprint = generation_fingerprint(call, &instance, &policy)?;
        plan_calls.push(ToolExecutionPlanCallV1 {
            tool_call_id: call.call.tool_call_id,
            ordinal: call.call.ordinal,
            dependency_tool_call_ids: dependencies
                .get(&call.call.tool_call_id)
                .cloned()
                .unwrap_or_default(),
            execution_atomic_group: atomic_groups.get(&call.call.tool_call_id).copied(),
            resolved_resource_claims: resources.scheduler_claims.clone(),
            timeout_ms: policy.timeout_ms,
            max_retries: policy.max_retries,
            local_concurrency_limit: instance.overrides.max_concurrent_attempts,
            entity_revision: instance.entity_revision,
        });
        runtime_calls.push(PlannedToolRuntimeCallV1 {
            validated_call: call.clone(),
            generation: ToolInstanceExecutionGenerationKeyV1 {
                tool_instance_id: instance.tool_instance_id,
                tool_version: instance.tool_version,
                execution_fingerprint,
                local_concurrency_limit: policy.local_concurrency_limit,
            },
            permission_scopes: compose_tool_operation_scopes_v1(
                &resources.permission_scopes,
                &runtime_required_scopes,
                &resolver_additional_scopes,
            ),
            risk_flags: resources.risk_flags,
            resolved_directories: resources.resolved_directories,
        });
    }
    plan_calls.sort_by_key(|call| call.ordinal);
    runtime_calls.sort_by_key(|call| call.validated_call.call.ordinal);
    let execution_plan = ToolExecutionPlanV1::new(
        admitted.batch.tool_batch_id,
        admitted.batch.registry_snapshot_ref.clone(),
        settings.revision,
        settings.content_hash.clone(),
        plan_calls,
    )
    .map_err(|_| ToolExecutionPlanningErrorV1::PlanInvalid)?;
    let mut not_executed = rejected.into_values().collect::<Vec<_>>();
    not_executed.sort_by_key(|rejection| rejection.ordinal);
    Ok(PlannedToolBatchV1 { execution_plan, runtime_calls, not_executed })
}

pub fn preflight_rejection_result_v1(
    rejection: &ToolPreflightRejectionV1,
    completed_at: aura_domain_common::UtcTimestamp,
    error: Option<aura_domain_common::ErrorEnvelope>,
) -> Result<ToolResultV1, ToolExecutionPlanningErrorV1> {
    if error.as_ref().map(|value| &value.code) != rejection.error_code.as_ref() {
        return Err(ToolExecutionPlanningErrorV1::AuthorityMismatch);
    }
    let result = ToolResultV1 {
        tool_call_id: rejection.tool_call_id,
        outcome: ToolResultOutcomeV1::NotExecuted { reason: rejection.reason, error },
        side_effect_state: SideEffectStateV1::NotStarted,
        attempts: Vec::new(),
        model_visible_result_ref: None,
        user_audit_result_ref: None,
        internal_diagnostic_ref: None,
        aggregate_side_effect_evidence_ref: None,
        started_at: None,
        completed_at,
    };
    result.validate().map_err(|_| ToolExecutionPlanningErrorV1::PlanInvalid)?;
    Ok(result)
}

#[derive(Clone, Copy)]
struct EffectiveExecutionPolicyV1 {
    timeout_ms: u64,
    max_retries: u8,
    local_concurrency_limit: u32,
}

fn resolve_execution_policy(
    settings: &ToolSettingsSnapshotV1,
    overrides: &ToolInstanceOverridesV1,
) -> Result<EffectiveExecutionPolicyV1, ToolExecutionPlanningErrorV1> {
    overrides.validate().map_err(|_| ToolExecutionPlanningErrorV1::AuthorityMismatch)?;
    Ok(EffectiveExecutionPolicyV1 {
        timeout_ms: overrides.timeout_ms.unwrap_or(settings.default_timeout_ms),
        max_retries: overrides.max_retries.unwrap_or(settings.max_retries),
        local_concurrency_limit: overrides
            .max_concurrent_attempts
            .unwrap_or(settings.max_concurrent_tool_calls),
    })
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct GenerationFingerprintPreimageV1<'a> {
    kind: &'static str,
    preimage_version: u32,
    tool_instance_id: aura_domain_common::ToolInstanceId,
    tool_version: &'a aura_tool_contract::ToolVersion,
    definition_fingerprint: &'a CanonicalHashV1,
    source_contract_fingerprint: &'a aura_tool_contract::ToolSourceContractFingerprintV1,
    instance_entity_revision: Revision,
    overrides: &'a ToolInstanceOverridesV1,
    timeout_ms: u64,
    max_retries: u8,
    local_concurrency_limit: u32,
}

fn generation_fingerprint(
    call: &ValidatedToolCallV1,
    instance: &ToolInstanceRecordV1,
    policy: &EffectiveExecutionPolicyV1,
) -> Result<CanonicalHashV1, ToolExecutionPlanningErrorV1> {
    canonical_sha256(&GenerationFingerprintPreimageV1 {
        kind: "aura.p07.tool_instance_execution_generation",
        preimage_version: 1,
        tool_instance_id: instance.tool_instance_id,
        tool_version: &instance.tool_version,
        definition_fingerprint: &call.definition.definition_fingerprint,
        source_contract_fingerprint: &instance.source_contract_fingerprint,
        instance_entity_revision: instance.entity_revision,
        overrides: &instance.overrides,
        timeout_ms: policy.timeout_ms,
        max_retries: policy.max_retries,
        local_concurrency_limit: policy.local_concurrency_limit,
    })
    .map_err(|_| ToolExecutionPlanningErrorV1::PlanInvalid)
}

fn validate_instance_for_call(
    call: &ValidatedToolCallV1,
    instance: &ToolInstanceRecordV1,
) -> Result<(), ToolExecutionPlanningErrorV1> {
    if instance.tool_instance_id != call.binding.tool_instance_id
        || instance.tool_id != call.definition.tool_id
        || instance.tool_version != call.definition.tool_version
        || instance.source_kind != call.binding.source_kind
        || instance.source_contract_fingerprint != call.binding.source_contract_fingerprint
        || instance.entity_revision.value() == 0
    {
        return Err(ToolExecutionPlanningErrorV1::AuthorityMismatch);
    }
    Ok(())
}

/// What waits for a
/// reconcile in the same batch is what depends on which task is current - the task's own records,
/// its todo, goal, plan and mode, and its completion. Ordinary tools run whatever the reconcile's
/// outcome, as its own description tells the model.
fn waits_for_reconcile(name: &str) -> bool {
    ["task_lineage.", "todo.", "goal.", "plan.", "normal.", "mode."]
        .iter()
        .any(|prefix| name.starts_with(prefix))
        || name == "workspace_init.complete"
}

fn derive_control_dependencies(
    calls: &[(ToolCallId, &str)],
) -> BTreeMap<ToolCallId, Vec<ToolCallId>> {
    let by_name = calls.iter().map(|(id, name)| (*name, *id)).collect::<BTreeMap<_, _>>();
    let reconcile = by_name.get("task_lineage.reconcile").copied();
    let completion_review = by_name.get("task_lineage.review_completion").copied();
    let todo_update = by_name.get("todo.update").copied();
    let mut output = BTreeMap::new();
    for (id, name) in calls {
        let mut dependencies = BTreeSet::new();
        if let Some(reconcile) = reconcile
            && *name != "task_lineage.reconcile"
            && waits_for_reconcile(name)
        {
            dependencies.insert(reconcile);
        }
        if *name == "normal.complete"
            && let Some(review) = completion_review
        {
            dependencies.insert(review);
        }
        if *name == "goal.check"
            && let Some(todo) = todo_update
        {
            dependencies.insert(todo);
        }
        output.insert(*id, dependencies.into_iter().collect());
    }
    output
}

fn derive_execution_atomic_groups(
    calls: &[aura_tool_contract::NormalizedToolCallV1],
    names: &BTreeMap<ToolCallId, &str>,
) -> BTreeMap<ToolCallId, u32> {
    let completion_members = calls
        .iter()
        .filter(|call| {
            matches!(
                names.get(&call.tool_call_id).copied(),
                Some("task_lineage.review_completion" | "normal.complete")
            )
        })
        .map(|call| call.tool_call_id)
        .collect::<Vec<_>>();
    if completion_members.len() == 2 {
        completion_members.into_iter().map(|id| (id, 1)).collect()
    } else {
        BTreeMap::new()
    }
}

fn propagate_atomic_group_rejections(
    groups: &BTreeMap<ToolCallId, u32>,
    calls: &[aura_tool_contract::NormalizedToolCallV1],
    rejected: &mut BTreeMap<ToolCallId, ToolPreflightRejectionV1>,
) {
    let rejected_groups = groups
        .iter()
        .filter_map(|(id, group)| rejected.contains_key(id).then_some(*group))
        .collect::<BTreeSet<_>>();
    for call in calls {
        if groups.get(&call.tool_call_id).is_some_and(|group| rejected_groups.contains(group)) {
            rejected.entry(call.tool_call_id).or_insert_with(|| ToolPreflightRejectionV1 {
                tool_call_id: call.tool_call_id,
                ordinal: call.ordinal,
                reason: ToolNotExecutedReasonV1::AtomicGroupNotExecuted,
                error_code: None,
                schema_validation: None,
                input_issue: None,
                input_diagnostic: None,
            });
        }
    }
}

fn propagate_dependency_rejections(
    dependencies: &BTreeMap<ToolCallId, Vec<ToolCallId>>,
    calls: &[aura_tool_contract::NormalizedToolCallV1],
    rejected: &mut BTreeMap<ToolCallId, ToolPreflightRejectionV1>,
) {
    loop {
        let rejected_ids = rejected.keys().copied().collect::<BTreeSet<_>>();
        let mut changed = false;
        for call in calls {
            if rejected.contains_key(&call.tool_call_id) {
                continue;
            }
            if dependencies
                .get(&call.tool_call_id)
                .is_some_and(|values| values.iter().any(|id| rejected_ids.contains(id)))
            {
                rejected.insert(
                    call.tool_call_id,
                    ToolPreflightRejectionV1 {
                        tool_call_id: call.tool_call_id,
                        ordinal: call.ordinal,
                        reason: ToolNotExecutedReasonV1::DependencyNotExecuted,
                        error_code: None,
                        schema_validation: None,
                        input_issue: None,
                        input_diagnostic: None,
                    },
                );
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
