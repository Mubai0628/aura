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
            // Skill folders are read before the store transaction; only the answer is recorded
            // inside it.
            let action_kind = &request.operation.content.action_kind;
            let archived = super::conversations::archived_refusal(service, call, request)?;
            let skill = if super::skill_tools::is_skill_tool(action_kind) {
                Some(super::skill_tools::prepare(
                    service,
                    call,
                    action_kind,
                    &request.operation.content.action,
                ))
            } else if super::plugin_tools::is_plugin_tool(action_kind) {
                Some(super::plugin_tools::prepare(
                    service,
                    call,
                    action_kind,
                    &request.operation.content.action,
                ))
            } else {
                None
            };
            service.journal.lock()?.database.unit_of_work(|tx| {
                let at = UtcTimestamp::now();
                if let Some(prepared) = skill {
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |_tx, _name, _arguments| {
                            prepared.map_err(aura_persistence_core::PersistenceCoreError::Invalid)
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
                } else if request.operation.content.action_kind == "computer.apps" {
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |tx, _name, arguments| {
                            super::computer::apps_result(service, call, tx, arguments)
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
                            super::registry::execute(service, tx, call, name, arguments, at)
                        },
                    )
                } else if let Some(refusal) = archived {
                    aura_persistence_v2::agent::tool_state::execute_with(
                        tx,
                        request,
                        &call.batch,
                        call.validated.call.tool_call_id,
                        at,
                        |_tx, _name, _arguments| {
                            Err(aura_persistence_core::PersistenceCoreError::Invalid(refusal))
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
    let query = input.to_query().map_err(|invalid| {
        aura_persistence_core::PersistenceCoreError::Invalid(refusal(invalid))
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
    // The one refusal a page makes is a resource target larger than a request may name.
    .map_err(|_| {
        aura_persistence_core::PersistenceCoreError::Invalid(
            "activity.query_target_over_limit: the target names more folders or entries than a \
             query may hold; name a smaller target, or query by session",
        )
    })?;
    serde_json::to_value(AgentActivityQueryResultV1 {
        contract_version: ACTIVITY_QUERY_CONTRACT_VERSION,
        page,
    })
    .map_err(Into::into)
}

/// What the model is told for an activity query that cannot be answered as written, under the
/// domain's own code for it.
fn refusal(invalid: aura_activity_domain::AgentActivityQueryInvalid) -> &'static str {
    use aura_activity_domain::AgentActivityQueryInvalid as Invalid;
    match invalid {
        Invalid::MissingTarget => {
            "activity.query_target_required: a query by resource needs the target it asks about; \
             pass target, or query by session"
        }
        Invalid::MissingGroup => {
            "activity.query_group_required: a query by group needs the tool group it asks about; \
             pass group, or query by session"
        }
        Invalid::MissingActivityId => {
            "activity.query_activity_id_required: an exact query needs the activity_id it asks \
             about; pass activity_id, or query by session"
        }
        Invalid::UnexpectedField => {
            "activity.query_unexpected_field: pass target only when querying by resource, group \
             only by group and activity_id only by exact; leave the others out"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each activity query the model wrote wrongly is named by the domain's code for it, with
    /// what to change.
    #[test]
    fn an_unanswerable_activity_query_names_its_code_and_the_fix() {
        use aura_activity_domain::AgentActivityQueryInvalid as Invalid;
        for invalid in [
            Invalid::MissingTarget,
            Invalid::MissingGroup,
            Invalid::MissingActivityId,
            Invalid::UnexpectedField,
        ] {
            let text = refusal(invalid);
            assert!(text.starts_with(&format!("{}: ", invalid.code())), "{text}");
            assert!(text.contains("pass ") || text.contains("leave "), "{text}");
        }
    }
}
