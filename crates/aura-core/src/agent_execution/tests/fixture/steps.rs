//! Model-step fixture progression references real completed tool history.
use super::*;
use aura_domain_common::PromptBundleSnapshotId;

pub(super) fn install_step(
    service: &crate::RuntimeServices,
    turn: &TurnTask,
    step: ModelStepId,
    registry: &ToolRegistrySnapshotV1,
    bundle: PromptBundleSnapshotId,
    insert_registry: bool,
) {
    let session = turn.session_id;
    let at = UtcTimestamp::now();
    let base = aura_context_fixtures::p06_prepared_context_fixture()
        .expect("construct the next provider context fixture");
    let canonical =
        aura_contract_schema::canonical_fixture().expect("construct provider request evidence");
    let prepared = aura_context_fixtures::p06_prepared_tool_context_fixture(
        aura_context_fixtures::P06ToolContextFixtureInputV1 {
            workspace_id: turn.workspace_id,
            session_id: session,
            turn_task_id: turn.turn_task_id,
            agent_run_id: turn.agent_run_id,
            model_step_id: step,
            model_profile_id: base.execution_snapshot.model_profile_id,
            provider_id: base.execution_snapshot.provider_id,
            endpoint_id: base.execution_snapshot.endpoint_id,
            tool_schema_set: registry.schema_set.clone(),
            effective_tool_bindings: registry.effective_bindings.clone(),
        },
    )
    .expect("prepare the provider context for this durable model step");
    // The test builder owns a separate fixture bundle; the actual Agent
    // turn keeps its frozen bundle, including its original user identity.
    let mut context = prepared.prepared.context.snapshot;
    context.prompt_bundle_snapshot_id = bundle;
    context.active_task_lineage_id = Some(turn.lineage_id);
    context = context.seal().expect("seal the context against the turn's frozen prompt bundle");
    let mut snapshot = prepared.execution_snapshot;
    snapshot.context_snapshot_id = context.id;
    snapshot.prompt_bundle_snapshot_id = bundle;
    let mut evidence = canonical
        .models
        .provider_request_evidence
        .expect("the canonical fixture contains provider request evidence");
    evidence.compatibility_removals.clear();
    evidence.historical_tool_call_wire_bindings.clear();
    evidence.adapter.api_format = snapshot.api_format;
    evidence.operation_context = aura_model_domain::TransportOperationContext::AgentModelAttempt {
        model_step_id: step,
        model_execution_snapshot_id: snapshot.id,
        context_snapshot_id: context.id,
        prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
    };
    evidence.tool_registry_snapshot_ref = Some(registry.snapshot_ref.clone());
    evidence.tool_name_bindings = registry
        .schema_set
        .tools
        .iter()
        .map(|schema| {
            aura_model_domain::ProviderToolNameBinding::from_snapshot(
                &registry.snapshot_ref,
                schema,
                aura_model_domain::provider_arguments_envelope_schema_v3(&schema.input_schema)
                    .expect("each registered tool schema has its provider envelope")
                    .1,
            )
        })
        .collect();
    evidence.tool_name_bindings.sort_by_key(|v| v.tool_definition.tool_id);
    service
        .broker
        .lock()
        .expect("lock the fixture's existing authority store to install the step")
        .database
        .unit_of_work(|tx| {
            if insert_registry {
                tool::insert_registry(tx, registry)?;
            }
            context::insert_snapshot(tx, &context)?;
            checkpoint::freeze(tx, step, context.id, at)?;
            model::insert_snapshot(tx, session, &snapshot)?;
            let previous = turns::step(tx, step)?;
            let mut next = previous.clone();
            next.state = ModelStepState::Running { execution_snapshot_id: snapshot.id };
            turns::save_step(tx, &previous, &next)?;
            let old = turns::load_run(tx, turn.agent_run_id)?;
            let mut next = old.clone();
            next.revision = Revision::new(old.revision.value() + 1);
            next.state = AgentRunState::ModelRunning { model_step_id: step };
            turns::save_run(tx, &old, &next)?;
            model::record_request(tx, snapshot.id, &evidence, at)
        })
        .expect("persist the frozen context, running step and provider request evidence");
}

impl Fixture {
    pub fn continue_after_tools(
        &mut self,
        batch: ToolBatchId,
        group: &aura_context_domain::HistoryAtomicGroupPreimageV1,
    ) {
        let registry = self.registry();
        let hash = group
            .group_ref()
            .expect("the completed batch has its durable history reference")
            .group_hash;
        let step = self
            .service()
            .broker
            .lock()
            .expect("lock the existing fixture authority")
            .database
            .unit_of_work(|tx| {
                let old = turns::load_run(tx, self.turn.agent_run_id)?;
                let mut next = old.clone();
                next.revision = Revision::new(old.revision.value() + 1);
                next.next_model_ordinal = old.next_model_ordinal + 1;
                next.state = AgentRunState::PreparingContext;
                turns::save_run(tx, &old, &next)?;
                lifecycle::prepare_step(
                    tx,
                    self.turn.agent_run_id,
                    AgentContinuation::ToolBatch { tool_batch_id: batch, history_group_hash: hash },
                    UtcTimestamp::now(),
                )
            })
            .expect("prepare a new model step against the real completed tool batch");
        install_step(self.service(), &self.turn, step.model_step_id, &registry, self.bundle, false);
        self.step = step.model_step_id;
    }
}

/// One envelope-3 call: the tool's own fields beside `aura_purpose`.
pub(super) fn flat_arguments(args: &serde_json::Value) -> String {
    let mut call = args.as_object().cloned().expect("tool arguments are a JSON object");
    call.insert("aura_purpose".into(), "Execute requested workspace task".into());
    serde_json::Value::Object(call).to_string()
}
