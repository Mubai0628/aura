use super::super::*;

/// The services that genuinely need the workspace runtime and tool configuration.
///
/// Grouped into one function so the dependency is stated once. Their
/// absence is a *capability* loss, recorded by the caller, not a Core failure.
pub(crate) struct DependentServices {
    pub(crate) commands: Option<Arc<command_execution::CommandExecution>>,
    pub(crate) agent_execution: Option<Arc<agent_execution::AgentExecution>>,
    pub(crate) agent: Option<Arc<agent_application::AgentApplication>>,
    pub(crate) p04: Option<P04Services>,
    pub(crate) p05: Option<P05Services>,
    pub(crate) security_sidecars: sidecars::SecuritySidecars,
    /// `None` when P05 never opened, because there is nothing to maintain then.
    pub(crate) p05_maintenance: Option<RuntimeBackgroundTask>,
    pub(crate) p07_maintenance: RuntimeBackgroundTask,
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn open_dependent_services(
    options: &CoreOptions,
    release: &CoreReleaseContext,
    settings: &Arc<SettingsFacade>,
    persistence: &Arc<PersistenceCoordinator>,
    broker: &Arc<broker::BrokerJournal>,
    broker_http: &Arc<aura_network_control::CoreHttpClientGateway>,
    activities: &activity::AgentActivities,
    agent_store: &Arc<agent_store::AgentStore>,
    sync: &aura_core_base::sync_boundary::SyncBoundary,
    activity_index: &aura_activity_runtime::ActivityIndex,
    activity_observer: &aura_activity_runtime::ActivityObserverHandle,
    p03: Arc<P03Services>,
    config_degradation: &config_degradation::SharedConfigDegradation,
    max_concurrent_tool_calls: u32,
) -> Result<DependentServices> {
    let agent_execution = Arc::new(agent_execution::AgentExecution::new_with_global_concurrency(
        &p03,
        Arc::clone(persistence),
        Arc::clone(broker),
        Arc::clone(agent_store),
        Arc::new(controlled_web::ControlledWeb::new(
            Arc::clone(persistence),
            Arc::clone(broker),
            Arc::clone(broker_http),
            activities.clone(),
            Arc::clone(agent_store),
        )),
        activities.clone(),
        activity_index.clone(),
        options.runtime_instance_id,
        max_concurrent_tool_calls,
    )?);
    agent_execution.control_resources.register_runtime(settings.paths(), release);
    // P04's security authority is its own capability. When the Security or
    // Aura domain is damaged it is refused with its real reason instead of
    // aborting Core.
    let p04 = match P04Services::open(
        Arc::clone(persistence),
        Arc::clone(&p03.fs),
        Arc::clone(&p03.workspace),
        settings,
        ElevatedBrokerOptions {
            helper_executable: release.elevated_helper_executable.clone(),
            helper_content_hash: release.elevated_helper_hash.clone(),
            component_manifest_path: release.manifest_path.clone(),
            release_descriptor: release.release_descriptor.clone(),
            runtime_instance_id: options.runtime_instance_id,
        },
        options.runtime_instance_id,
        p03.publication_pool(),
        activity_index.clone(),
        activity_observer.clone(),
    ) {
        Ok(services) => Some(services),
        Err(error) => {
            config_degradation.degrade(
                ConfigDependentCapability::SecurityAuthority,
                Some(ConfigModule::Security),
                ConfigDependentCapability::SecurityAuthority.unavailable_code(),
                true,
            );
            append_core_log(
                "security_authority_config_unavailable",
                json!({
                    "capability": ConfigDependentCapability::SecurityAuthority.key(),
                    "fault_code": ConfigDependentCapability::SecurityAuthority.unavailable_code(),
                    "reason": error.to_string(),
                }),
            );
            None
        }
    };
    if let Some(p04) = &p04 {
        p04.bind_turn_journal(Arc::clone(&agent_execution.journal));
    }
    persistence.bind_tool_credential_scope_owner(
        Arc::clone(agent_store) as Arc<dyn aura_persistence::ToolCredentialScopeSource>
    )?;
    // P05's model authority is its **own** capability. This used to
    // be a bare `?`, so a damaged `models.json` aborted this whole function and
    // silently discarded an already-opened P04 — the user lost the security authority
    // as a side effect of an unrelated model fault, with no record that P04 had ever
    // been available.
    // The `AURA_S05R2_ABORT_CHAIN_COUNTERFACTUAL` switch that used to
    // live here has been **removed from product code**. It restored the pre-repair bare
    // `?` chain, which discarded every already-opened capability when one failed — a
    // product-path mutation of the isolation guarantee. The counterfactual is reproducible by
    // temporarily editing this function.
    let p05 = match P05Services::open(
        Arc::clone(persistence),
        Arc::clone(settings),
        release.release_descriptor.bundle_namespace.clone(),
        sync.clone(),
    ) {
        Ok(services) => Some(services),
        Err(error) => {
            config_degradation.degrade(
                ConfigDependentCapability::ModelAuthority,
                Some(ConfigModule::Models),
                ConfigDependentCapability::ModelAuthority.unavailable_code(),
                true,
            );
            append_core_log(
                "model_authority_config_unavailable",
                json!({
                    "capability": ConfigDependentCapability::ModelAuthority.key(),
                    "fault_code": ConfigDependentCapability::ModelAuthority.unavailable_code(),
                    "reason": error.to_string(),
                }),
            );
            None
        }
    };
    // Maintenance follows the service it maintains: with no P05 there is nothing to
    // maintain, so no task is started rather than one over a service that never opened.
    let p05_maintenance = match &p05 {
        Some(p05) => Some(spawn_p05_maintenance(settings, p05.clone(), sync.clone())?),
        None => None,
    };
    let p07_maintenance = spawn_p07_maintenance(settings, Arc::clone(persistence), sync.clone())?;
    let security_sidecars = sidecars::SecuritySidecars::deferred(options, release);
    // The command backend and the Agent application are separate
    // capabilities as well. Each was a bare `?`, so a command-backend failure discarded
    // P04 **and** P05, and an Agent failure discarded the command backend. Each refusal
    // is now recorded on its own, and every capability that really opened is kept.
    let commands = match command_execution::CommandExecution::open(
        Arc::clone(persistence),
        Arc::clone(broker),
        Arc::clone(agent_store),
        activity_index.clone(),
        options.runtime_instance_id,
        &security_sidecars.readiness,
        release,
    ) {
        Ok(commands) => Some(Arc::new(commands)),
        Err(error) => {
            config_degradation.degrade(
                ConfigDependentCapability::ToolPolicy,
                Some(ConfigModule::Tools),
                ConfigDependentCapability::ToolPolicy.unavailable_code(),
                true,
            );
            append_core_log(
                "command_backend_config_unavailable",
                json!({
                    "capability": ConfigDependentCapability::ToolPolicy.key(),
                    "fault_code": ConfigDependentCapability::ToolPolicy.unavailable_code(),
                    "reason": error.to_string(),
                }),
            );
            None
        }
    };
    // The Agent application needs a real model authority; with no P05 there is no
    // authority to read, so no Agent is created and the reason is already recorded.
    let agent = match (&commands, &p05) {
        (Some(commands), Some(p05)) => {
            match agent_application::AgentApplication::new_for_process_startup(
                service_rebuild::StartupRecovery::for_process_startup(),
                agent_execution.clone(),
                commands.clone(),
                p05.clone(),
                Arc::clone(settings),
                Arc::clone(agent_store),
            ) {
                Ok(agent) => {
                    let agent = Arc::new(agent);
                    // Only a really-created Agent is restored and backfilled, so neither
                    // runs against a service that does not exist.
                    agent.restore_waits().await?;
                    agent.start_derived_backfill();
                    Some(agent)
                }
                Err(error) => {
                    config_degradation.degrade(
                        ConfigDependentCapability::ToolPolicy,
                        Some(ConfigModule::Tools),
                        ConfigDependentCapability::ToolPolicy.unavailable_code(),
                        true,
                    );
                    append_core_log(
                        "agent_application_config_unavailable",
                        json!({
                            "capability": ConfigDependentCapability::ToolPolicy.key(),
                            "fault_code": ConfigDependentCapability::ToolPolicy.unavailable_code(),
                            "reason": error.to_string(),
                        }),
                    );
                    None
                }
            }
        }
        // No command backend or no model authority means no Agent: the execution layer
        // writes through the former and reads the latter. This is stated rather than
        // left to fail later with a less honest message.
        _ => None,
    };
    Ok(DependentServices {
        commands,
        agent_execution: Some(agent_execution),
        agent,
        p04,
        p05,
        security_sidecars,
        p05_maintenance,
        p07_maintenance,
    })
}
