use super::*;
pub(super) fn inspect(
    service: &AgentExecution,
    call: &CallContext,
) -> anyhow::Result<serde_json::Value> {
    Ok(aura_persistence_v2::agent::tool_state::inspect(
        service.journal.lock()?.database.connection(),
        &call.batch,
    )?)
}
pub(super) fn execute(
    service: &AgentExecution,
    call: &CallContext,
    request: &DispatchRequest,
) -> anyhow::Result<()> {
    service.persistence.with_session_permission_authority(
        call.snapshot.session_id,
        |mode| -> anyhow::Result<()> {
            anyhow::ensure!(
                mode.mode == call.snapshot.mode
                    && mode.revision == call.snapshot.session_permission_revision,
                "agent.mode_changed"
            );
            service.verify_source(call)?;
            let _activity = aura_fs_tools::observe_file_execution(&[]);
            // Skill folders are read before the store transaction; only the turn's loaded
            // set is recorded inside it.
            let action_kind = &request.operation.content.action_kind;
            let skill = super::skill_tools::is_skill_tool(action_kind).then(|| {
                super::skill_tools::prepare(
                    service,
                    call,
                    action_kind,
                    &request.operation.content.action,
                )
            });
            service.journal.lock()?.database.unit_of_work(|tx| {
                let at = UtcTimestamp::now();
                if let Some(prepared) = skill {
                    let run = call.batch.invocation_identity.agent_run_id;
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |tx, _name, _arguments| {
                            let prepared = prepared
                                .map_err(aura_persistence_core::PersistenceCoreError::Invalid)?;
                            if let Some(loaded) = &prepared.loaded {
                                aura_persistence_v2::agent::skills::set_loaded(
                                    tx, run, loaded, at,
                                )?;
                            }
                            Ok(prepared.output)
                        },
                    )
                } else if request.operation.content.action_kind == "chrome.tabs" {
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |_tx, _name, _arguments| {
                            super::chrome::tabs_result(service, call.snapshot.session_id).map_err(
                                |_| {
                                    aura_persistence_core::PersistenceCoreError::Invalid(
                                        "chrome.tabs",
                                    )
                                },
                            )
                        },
                    )
                } else if request.operation.content.action_kind == "agent.activity.query" {
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |_tx, _name, arguments| query_activity(service, call, arguments),
                    )
                } else if request.operation.content.action_kind.starts_with("tools.") {
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |tx, name, arguments| {
                            super::registry::execute(tx, call, name, arguments, at)
                        },
                    )
                } else {
                    aura_persistence_v2::agent::tool_state::execute(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                    )
                }
            })?;
            Ok(())
        },
    )?
}

fn query_activity(
    service: &AgentExecution,
    call: &CallContext,
    arguments: &serde_json::Value,
) -> aura_persistence_core::StoreResult<serde_json::Value> {
    use aura_activity_domain::*;
    let input: AgentActivityQueryInput = serde_json::from_value(arguments.clone())?;
    let query = input.to_query().map_err(|_| {
        aura_persistence_core::PersistenceCoreError::Invalid("invalid activity query")
    })?;
    let mut view = aura_activity_runtime::ActivityVisibility::for_session(
        service.runtime,
        call.snapshot.session_id,
        call.snapshot.workspace_id,
    );
    let references = service.task_resource_references(call.batch.invocation_identity.agent_run_id);
    view.relevant_resources = references
        .iter()
        .take(MAX_ACTIVITY_RELEVANT_HINTS)
        .map(|resource| resource.target.clone())
        .collect();
    view.relevant_targets =
        crate::agent_activity::query::keys(references.iter().map(|resource| &resource.target));
    let page = crate::agent_activity::query::page(
        &service.activity_index,
        view,
        query,
        input.expected_revision,
        input.offset,
        input.effective_limit(),
        true,
    )
    .map_err(|_| {
        aura_persistence_core::PersistenceCoreError::Invalid("invalid activity query target")
    })?;
    serde_json::to_value(AgentActivityQueryResultV1 {
        contract_version: ACTIVITY_QUERY_CONTRACT_VERSION,
        page,
    })
    .map_err(Into::into)
}
