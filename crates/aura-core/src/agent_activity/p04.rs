//! Execution facts for the p04 publisher.
use super::*;

/// Publishes one real P04 external publication.
///
/// Has its own lifecycle/publication identity and no Agent run, so the record carries
/// exactly that. The target is reported as unproven with its path as the display name: the
/// Export path resolves its destination through the external-grant resolver, and the
/// verified identity it has at this point belongs to the grant, not to a durable object
/// reference this module may hold on to.
pub(crate) fn publish_p04_publication_activity(
    services: &crate::P04Services,
    workspace_id: aura_domain_common::WorkspaceId,
    session_id: aura_domain_common::SessionId,
    correlation_id: aura_domain_common::CorrelationId,
    operation_id: Option<aura_domain_common::SecurityOperationId>,
    resources: &[aura_workspace_domain::ResourceRequest],
    tool_name: &str,
) -> AgentActivityGuard {
    use aura_activity_domain::*;
    let execution = ActivityExecutionRef::P04FilePublication { correlation_id, operation_id };
    let generation = p04_generation(services);
    let id = ActivityId::derive(ActivityKind::P04Publication, &execution, generation);
    let record = ActivityRecord {
        id,
        kind: ActivityKind::P04Publication,
        execution,
        generation,
        tool_id: aura_domain_common::ToolId::from_stable_name(tool_name),
        tool_name: tool_name.into(),
        private_source: if tool_name == "artifact.export" {
            ActivityPrivateSource::WorkspaceMetadata
        } else {
            ActivityPrivateSource::PersonalData
        },
        tool_group: ActivityToolGroup::FileWrite,
        owner: ActivityOwnerScope {
            session_id,
            workspace_id,
            agent_run_id: None,
            turn_task_id: None,
            model_step_id: None,
            runtime_instance_id: services.activity_runtime_instance_id(),
        },
        resources: self::resources::project(resources),
        parameters: Vec::new(),
        started_at: aura_domain_common::UtcTimestamp::now(),
        runtime_instance_id: services.activity_runtime_instance_id(),
        published_at_revision: aura_domain_common::Revision::INITIAL,
    };
    let outcome = services.activity_index().publish(record);
    services.activity_observer().observe(outcome);
    AgentActivityGuard::new(services.activity_index().clone(), id, generation)
}

pub(crate) fn p04_file_observer(
    services: &crate::P04Services,
    workspace_id: aura_domain_common::WorkspaceId,
    session_id: aura_domain_common::SessionId,
    correlation_id: aura_domain_common::CorrelationId,
) -> aura_fs_tools::FileExecutionObserver {
    let services = services.clone();
    std::sync::Arc::new(move |requests| {
        let guard = publish_p04_publication_activity(
            &services,
            workspace_id,
            session_id,
            correlation_id,
            None,
            requests,
            "artifact.export",
        );
        Box::new(guard)
    })
}

fn p04_generation(services: &crate::P04Services) -> u32 {
    let digest = aura_domain_common::jcs::canonical_sha256(&(
        "aura.activity.p04_generation.v1",
        services.activity_runtime_instance_id().to_string(),
    ));
    digest
        .map(|hash| u32::from_str_radix(hash.hex().get(..8).unwrap_or("0"), 16).unwrap_or(1).max(1))
        .unwrap_or(1)
}
