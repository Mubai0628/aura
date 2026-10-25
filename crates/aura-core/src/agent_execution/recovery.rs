//! Restore only facts previously admitted by Core. Live policy and source checks
//! still run before any unattempted call; an existing Attempt is never replayed.
use super::*;
use aura_persistence_v2::{agent::checkpoint, tool};
use aura_tool_scheduler::*;
use std::collections::BTreeMap;

struct StoredAuthority<'a>(&'a ToolBatchRequestV1);
impl BatchAdmissionAuthorityV1 for StoredAuthority<'_> {
    fn verify_batch_admission(
        &self,
        batch: &ToolBatchRequestV1,
    ) -> Result<(), BatchAdmissionAuthorityIssueV1> {
        if self.0 == batch {
            Ok(())
        } else {
            Err(BatchAdmissionAuthorityIssueV1::InvocationBoundaryInvalid)
        }
    }
}
/// The package and MCP tool records a stored snapshot names, as they were stored
/// with it.
fn stored_package_tools(
    db: &aura_persistence_core::Connection,
    registry: &ToolRegistrySnapshotV1,
) -> anyhow::Result<crate::agent_execution::package_tools::SnapshotPackageTools> {
    let mut tools = Vec::new();
    for entry in registry
        .catalog_entries
        .iter()
        .filter(|entry| entry.source != aura_tool_contract::ToolSourceKindV1::BuiltIn)
    {
        if let (Some(definition), Some(instance)) = (
            tool::definitions::get(db, entry.tool_id, &entry.tool_version)?,
            tool::definitions::instance(db, entry.tool_instance_id)?,
        ) && !super::operation::is_builtin(&definition)
        {
            tools.push((definition, instance));
        }
    }
    Ok(tools)
}

impl AgentExecution {
    /// Rebuilds an already-admitted batch from durable facts. Never replays a backend.
    ///
    /// Durable admission facts are read under one journal lease. External authority is
    /// refreshed after releasing that lease: its file-consent origin also reads this journal.
    /// The whole rebuild remains inside admission's bounded Agent Store section.
    pub fn resume_batch(&self, id: ToolBatchId) -> anyhow::Result<AdmittedBatch> {
        let store = self.journal.lock()?;
        let db = store.database.connection();
        let stored = tool::execution::batch(db, id)?;
        let input = tool::admission::load(db, id)?
            .ok_or_else(|| anyhow::anyhow!("agent.admission_missing"))?;
        let registry =
            tool::registry(db, stored.batch.registry_snapshot_ref.tool_registry_snapshot_id)?
                .ok_or_else(|| anyhow::anyhow!("agent.registry_missing"))?;
        checkpoint::verify_frozen_batch(db, &stored.batch)?;
        anyhow::ensure!(
            registry.snapshot_ref == stored.batch.registry_snapshot_ref
                && aura_persistence_v2::security::snapshot(db, input.permission_snapshot.id)?
                    .as_ref()
                    == Some(&input.permission_snapshot),
            "agent.restored_authority_changed"
        );
        let release = aura_tool_registry::agent_release::release()?;
        // Package tools restore from the records stored with the snapshot, never
        // from the live install. An unattempted call re-checks the install at dispatch.
        let package = stored_package_tools(db, &registry)?;
        let admitted = admit_tool_batch(
            stored.batch.clone(),
            &registry,
            release
                .manifest()
                .entries
                .iter()
                .map(|e| e.definition.clone())
                .chain(package.iter().map(|(definition, _)| definition.clone()))
                .collect(),
            &StoredAuthority(&stored.batch),
        )?;
        let admitted = match admitted {
            ToolBatchAdmissionOutcomeV1::Admitted(v) => *v,
            ToolBatchAdmissionOutcomeV1::BatchRejected { calls, .. }
                if stored.plan.calls.is_empty() =>
            {
                AdmittedToolBatchV1 {
                    batch: stored.batch.clone(),
                    validated_calls: vec![],
                    not_executed: calls,
                }
            }
            _ => anyhow::bail!("agent.restored_definition_changed"),
        };
        let mut calls = BTreeMap::new();
        let mut planning_inputs = vec![];
        for validated in &admitted.validated_calls {
            let (definition, instance) = match package
                .iter()
                .find(|(definition, _)| definition.tool_id == validated.definition.tool_id)
            {
                Some((definition, instance)) => (definition, instance),
                None => {
                    let entry = operation::entry(validated)?;
                    (&entry.definition, &entry.instance)
                }
            };
            anyhow::ensure!(
                tool::definitions::get(db, definition.tool_id, &definition.tool_version)?.as_ref()
                    == Some(definition),
                "agent.restored_definition_changed"
            );
            let resources = if let Some(plan) =
                stored.plan.calls.iter().find(|c| c.tool_call_id == validated.call.tool_call_id)
            {
                let operation = input
                    .operations
                    .get(&plan.tool_call_id)
                    .ok_or_else(|| anyhow::anyhow!("agent.restored_operation_missing"))?
                    .clone();
                operation.validate()?;
                calls.insert(
                    plan.tool_call_id,
                    CallContext {
                        batch: stored.batch.clone(),
                        registry: (*registry).clone(),
                        validated: validated.clone(),
                        snapshot: input.permission_snapshot.clone(),
                        verified_external_capabilities: vec![],
                        verified_external_grants: vec![],
                        operation,
                        observed_resources: Arc::new(std::sync::Mutex::new(Vec::new())),
                    },
                );
                ToolCallResourcePreflightV1::Resolved(ResolvedPackageResourceSetV1 {
                    scheduler_claims: plan.resolved_resource_claims.clone(),
                    permission_scopes: vec![],
                    risk_flags: vec![],
                    resolved_directories: Default::default(),
                })
            } else {
                let call = tool::execution::call(db, validated.call.tool_call_id)?;
                let Some(ToolResultV1 {
                    outcome: ToolResultOutcomeV1::NotExecuted { reason, error },
                    ..
                }) = call.result
                else {
                    anyhow::bail!("agent.restored_preflight_missing");
                };
                ToolCallResourcePreflightV1::Rejected(PackageResourcePreflightRejectionV1 {
                    reason,
                    error_code: error.map_or_else(
                        || ToolResourceErrorCode::IdentityResolutionFailed.error_code(),
                        |e| e.code,
                    ),
                    claim_key: None,
                })
            };
            planning_inputs.push(FrozenToolCallPlanningInputV1 {
                tool_call_id: validated.call.tool_call_id,
                instance: instance.clone(),
                resources,
                runtime_required_scopes: vec![],
                resolver_additional_scopes: vec![],
            });
        }
        let planned =
            build_tool_execution_plan_v1(&admitted, &registry, &input.settings, planning_inputs)?;
        anyhow::ensure!(
            planned.execution_plan == stored.plan && calls.len() == input.operations.len(),
            "agent.restored_plan_changed"
        );
        drop(store);
        for call in calls.values_mut() {
            call.verified_external_capabilities = self.external_authority(&call.operation);
            call.verified_external_grants = self.external_authority_slices(&call.operation);
        }
        Ok(AdmittedBatch {
            batch: stored.batch,
            planned,
            snapshot: input.permission_snapshot,
            calls,
        })
    }
}
