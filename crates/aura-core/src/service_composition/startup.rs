use crate::*;

#[cfg(feature = "p08-contract-driver")]
#[path = "../l01_e2e_fixture.rs"]
mod l01_e2e_fixture;

pub(crate) async fn initialize_services_at(
    options: &CoreOptions,
    release: &CoreReleaseContext,
    paths: AuraPaths,
) -> Result<RuntimeServices> {
    paths.ensure_layout().context("failed to create Aura data layout")?;
    append_startup_stage("paths_layout_ready");
    let broker = Arc::new(
        broker::BrokerJournal::open(&paths.data_dir.join("v2-execution"))
            .context("failed to open isolated V2 Broker execution store")?,
    );
    append_startup_stage("broker_journal_opened");
    // One registry for the whole process, created **before** the config
    // consumers that report into it. It owns no configuration and no permission;
    // it records which capability lost its configuration and why, so a fault in
    // one domain costs that capability instead of all of Core.
    let config_degradation = config_degradation::SharedConfigDegradation::default();
    // A configuration fault here used to be `?`, which took away the
    // entire runtime — sessions, status, stop and repair with it. The fault is
    // now recorded against the one capability it really disables
    // (`ConfigTransactionRecovery`), and startup continues so everything that
    // does not depend on the unresolved journal stays alive.
    match ConfigTransactionJournal::new(&paths).recover_with(&mut CoreConfigTransactionRecovery) {
        Ok(actions) => {
            if !actions.is_empty() {
                append_core_log(
                    "config_transaction_recovered",
                    json!({ "recovered_actions": actions.len() }),
                );
            }
        }
        Err(error) => {
            // The stable code and the domain are logged; the raw failure stays in
            // the configuration layer's own error, never in ordinary output.
            append_core_log(
                "config_transaction_recovery_degraded",
                json!({
                    "capability": ConfigDependentCapability::ConfigTransactionRecovery.key(),
                    "fault_code": "core.config_recovery.unresolved",
                    "reason": error.to_string(),
                }),
            );
            config_degradation.degrade(
                ConfigDependentCapability::ConfigTransactionRecovery,
                None,
                "core.config_recovery.unresolved",
                false,
            );
        }
    }
    append_startup_stage("config_journal_recovered");
    let mut validators = BTreeMap::<ConfigModule, Arc<dyn ModuleConfigValidator>>::new();
    validators.insert(ConfigModule::Tools, Arc::new(P03ToolsConfigValidator));
    validators.insert(ConfigModule::Security, Arc::new(P04SecurityConfigValidator));
    validators.insert(ConfigModule::Models, Arc::new(ModelConfigValidator));
    // `open_with_validators` already isolates each domain — it records a
    // domain's own load failure against that domain and still opens the other
    // fourteen. It returns `Err` only when the *layout itself* cannot be
    // prepared, because with no directories there is no domain to be independent
    // of; that is a genuine Core fault rather than a configuration-domain fault.
    let settings = SettingsFacade::open_with_validators(paths.clone(), validators)
        .context("failed to initialize config")?;
    // A domain that could not be opened at startup is reported through the shared
    // registry, so a consumer refuses its own dependent capability honestly
    // instead of reading a fabricated default.
    for module in settings.degraded_domains() {
        let health = settings.health(module);
        let fault_code =
            health.original_fault.map_or("config.domain.unknown", |fault| fault.code());
        for capability in ConfigDependentCapability::ALL {
            if capability.domains().contains(&module) {
                config_degradation.degrade(capability, Some(module), fault_code, true);
            }
        }
    }
    if !settings.recovery_scope_known() {
        // The affected set itself could not be established. The user's confirmed
        // protection applies: keep view, stop and repair, suspend new calls that
        // depend on the suspect configuration, and do not mark it Ready.
        config_degradation.degrade(
            ConfigDependentCapability::ConfigTransactionRecovery,
            None,
            config_degradation::BUNDLE_SCOPE_UNRESOLVED,
            false,
        );
    }
    if !settings.degraded_domains().is_empty() || !settings.recovery_held_domains().is_empty() {
        append_core_log(
            "config_domains_degraded",
            json!({
                "degraded": settings.degraded_summary(),
                "recovery_scope_known": settings.recovery_scope_known(),
            }),
        );
    }
    append_startup_stage("settings_loaded");
    // The Skill folders' first reading after a build or an install runs beside the start.
    crate::skills::preread_in_background(Arc::clone(&settings));
    // The recovery entry is created beside the settings facade it repairs
    // and the degradation registry it updates, so both halves of "recover the
    // configuration and stop reporting the capability as unavailable" share one
    // owner.
    let config_recovery = config_recovery::ConfigRecoveryEntry::new(
        Arc::clone(&settings),
        Arc::clone(&config_degradation),
    );
    let config_watcher = settings.start_watching().context("failed to start config hot reload")?;
    append_startup_stage("config_watcher_started");
    let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
        paths.database_path,
        paths.artifact_objects_dir,
        options.runtime_instance_id,
    ))
    .context("failed to initialize persistence")?
    {
        PersistenceStartup::Ready(persistence) => Arc::new(persistence),
        PersistenceStartup::RecoveryRequired(recovery) => {
            bail!("database migration requires recovery: {}", recovery.reason);
        }
    };
    append_startup_stage("persistence_started");
    let tool_secret_recovery =
        tool_credential_service::ToolCredentialService::new(Arc::clone(&persistence)).recover(None);
    if tool_secret_recovery.as_ref().map_or(true, |outcomes| outcomes.iter().any(Result::is_err)) {
        append_core_log(
            "tool_credential_recovery_pending",
            json!({"code":"tool.credential.recovery_required"}),
        );
    }

    #[cfg(feature = "p08-contract-driver")]
    p08_contract_driver::seed_isolated_workspace_session_v1(
        options.p08_contract_driver.as_ref(),
        &persistence,
    )
    .context("failed to seed isolated E2E Workspace and Session fixture")?;
    match aura_tool_registry::embedded_builtin_tool_definition_set_v1() {
        Ok(_) => {
            let materialized = persistence
                .materialize_signed_builtin_release(UtcTimestamp::now(), CorrelationId::new())
                .context("failed to materialize signed Built-in Tool release")?;
            if let Err(error) =
                persistence.publish_tool_projection_changes(&materialized.projection_changes)
            {
                append_core_log(
                    "signed_builtin_projection_publish_failed",
                    json!({ "reason": error.to_string() }),
                );
            }
            append_core_log(
                "signed_builtin_release_ready",
                json!({
                    "changed": materialized.changed,
                    "release_revision": materialized.release_revision,
                    "signed_release_hash": materialized.signed_release_hash,
                    "eligible_instance_ids": materialized.eligible_instance_ids,
                    "quarantined_instance_ids": materialized.quarantined_instance_ids,
                }),
            );
        }
        Err(error) => {
            append_core_log(
                "signed_builtin_release_quarantined",
                json!({ "reason": error.to_string() }),
            );
        }
    }
    append_startup_stage("builtin_release_ready");
    let builtin_views = persistence
        .rebuild_agent_builtin_configuration_projections()
        .context("failed to refresh carried Built-in configuration views")?;
    persistence
        .publish_tool_projection_changes(&builtin_views)
        .context("failed to publish carried Built-in configuration views")?;
    let context_recovery = persistence
        .reconcile_context_after_core_restart(CorrelationId::new(), UtcTimestamp::now())
        .context("failed to reconcile P06 context state after Core restart")?;
    append_core_log(
        "p06_context_restart_reconciled",
        json!({
            "failed_before_model_attempt": context_recovery.failed_before_model_attempt,
            "failed_before_transport": context_recovery.failed_before_transport,
            "pre_dispatch_attempts_cancelled": context_recovery.pre_dispatch_attempts_cancelled,
            "transport_failures_preserved": context_recovery.transport_failures_preserved,
            "transport_cancellations_preserved": context_recovery.transport_cancellations_preserved,
            "logical_attempt_failures_preserved": context_recovery.logical_attempt_failures_preserved,
            "completed_outputs_left_unaccepted": context_recovery.completed_outputs_left_unaccepted,
            "unknown_side_effects": context_recovery.unknown_side_effects,
            "manual_requests_terminalized": context_recovery.manual_requests_terminalized,
            "prompt_inspector_projections_published": context_recovery.prompt_inspector_projections_published,
        }),
    );
    let _ = persistence
        .workspace_session_projection()
        .context("failed to rebuild Workspace/Session projection")?;
    append_startup_stage("p06_reconciled");
    // The one run-information index of this process, created before the services that
    // publish into it. It is passed **by clone** to each real owner - the workspace/buffer store,
    // the Shell backend, Main's file entry, P04 and the Agent execution layer - so there is one
    // table for the whole process rather than one per consumer. It owns nothing that can execute.
    let activity_index = aura_activity_runtime::ActivityIndex::default();
    let activity_observer = aura_activity_runtime::ActivityObserverHandle::default();
    // P03's workspace-files capability genuinely needs `tools.json`, so an
    // unreadable Tools domain really does disable it. What must **not** happen is
    // what used to happen: `?` here aborted `initialize_services_at`, which
    // `classify_startup` turned into `RecoveryRequired`, which disabled the whole
    // product — sessions, status, stop and repair.
    //
    // The capability is therefore degraded, its real reason is recorded, and Core
    // continues. `p03` is `None`, and every consumer of `p03` below is reached
    // only through the points that genuinely require it; the unrelated control
    // and projection paths do not.
    let p03_config = match P03Services::open(
        Arc::clone(&persistence),
        Arc::clone(&broker),
        &settings,
        activity_index.clone(),
        activity_observer.clone(),
        options.runtime_instance_id,
    ) {
        Ok(services) => Some(Arc::new(services)),
        Err(error) => {
            let fault_code = ConfigDependentCapability::ToolPolicy.unavailable_code();
            config_degradation.degrade(
                ConfigDependentCapability::ToolPolicy,
                Some(ConfigModule::Tools),
                fault_code,
                true,
            );
            append_core_log(
                "p03_workspace_files_config_unavailable",
                json!({
                    "capability": ConfigDependentCapability::ToolPolicy.key(),
                    "fault_code": fault_code,
                    "reason": error.to_string(),
                }),
            );
            None
        }
    };
    append_startup_stage("p03_services_open");
    let broker_http = Arc::new(aura_network_control::CoreHttpClientGateway::new(
        Arc::new(aura_network_control::PersistenceNetworkGrantStore::new((*persistence).clone())),
        None,
        aura_network_control::NetworkControlOptions::default(),
    ));
    #[cfg(feature = "p08-contract-driver")]
    l01_e2e_fixture::install(&broker_http);
    let activities = activity::AgentActivities::default();
    // The one activity publisher set, retained for a live supply of
    // the execution layer so it publishes into the same set the Shell, Main and buffer paths use.
    let activities_for_services = activities.clone();
    // The one bounded synchronous boundary of the process, and the one Agent Store owner built on
    // it, exist **before** the applications built on them: the web application and the execution
    // layer share the same `Arc<AgentStore>` the application layer uses, instead of opening a
    // second admission capacity or taking `journal.lock()` inline on a reactive task. The web
    // application's synchronous sections run on it too.
    let sync = crate::sync_boundary::SyncBoundary::open();
    let agent_store = Arc::new(agent_store::AgentStore::new(Arc::clone(&broker), sync.clone()));
    // The controlled-web application is built inside
    // `open_dependent_services`, on the same boundary and the same Agent Store
    // owner, because it is part of the dependent chain: it is handed to the
    // execution layer, which needs the workspace runtime.
    // The committed execution strategy is applied at construction, so a restarted
    // runtime enforces the stored global capacity before any request or UI read.
    //
    // This used to be
    // `.unwrap_or_else(|_| ToolSettingsSnapshotV1::default())`. That fabricates a
    // **legitimate authority out of a read failure**. A default is the right
    // answer when the domain genuinely has no file yet — first use, which the
    // user confirmed may create a validated default, and which `aura-config`
    // already does behind the domain's own validator. It is the wrong answer when
    // the read *failed*: the runtime then does not know whether the stored policy
    // was more or less restrictive, so serving a default silently widens or
    // narrows authority. The read failure is reported instead, and the tool
    // policy capability is marked unavailable; every other capability and every
    // control path keeps working and the fault is repairable.
    let committed_tool_settings = match settings.tool_settings_snapshot() {
        Ok(snapshot) => Some(snapshot),
        Err(error) => {
            let fault_code = config_degradation::config_fault_code(&error);
            config_degradation.degrade(
                ConfigDependentCapability::ToolPolicy,
                error.module(),
                fault_code,
                true,
            );
            append_core_log(
                "tool_policy_config_unavailable",
                json!({
                    "capability": ConfigDependentCapability::ToolPolicy.key(),
                    "domain": error.module().map(ConfigModule::key),
                    "fault_code": fault_code,
                    // Whether the recovery entry can act on this fault. Telling the
                    // user to run recovery for a fault recovery cannot fix would be
                    // an unactionable instruction.
                    "recoverable": config_recovery::is_recoverable_configuration_fault(&error),
                }),
            );
            None
        }
    };
    // With no legitimate tool policy there is no safe concurrency number to
    // invent, so the execution layer is started with the **most conservative**
    // capacity the contract admits rather than a fabricated default policy. A
    // capacity of one cannot widen any authority: it can only serialize work.
    let max_concurrent_tool_calls =
        committed_tool_settings.as_ref().map_or(1, |snapshot| snapshot.max_concurrent_tool_calls);
    // The one run-information index of this process. It is created here, beside the other
    // process-wide owners, and shared rather than rebuilt per consumer, so the Shell publisher, the
    // Main publisher and the context fragment all read the same table. It owns nothing that can
    // execute: no permit, no lock over a resource, no admission decision.
    //
    // P03, the execution layer, P04 and P05 are the **dependent** chain.
    // They genuinely need the workspace runtime and the tool/model configuration,
    // so when that configuration is gone they are refused — but as a *capability*,
    // not as the whole runtime. `broker`, `persistence`, `settings`, the config
    // watcher, the control paths and the projections above are already built and
    // stay alive, which is what "still accepts unrelated control" means.
    //
    // This is the shape the brief requires: mark genuinely dependent capabilities
    // unavailable while the rest, plus status/stop/repair, stay alive.
    //
    // `p03_config` is cloned for the dependent chain so the same `Arc` is both
    // handed to the consumers and kept in `RuntimeServices` as the service's one
    // owner. Cloning the `Arc` (rather than moving the field) is what keeps the
    // record and the consumers pointing at the same runtime.
    let dependent_p03 = p03_config.clone();
    // Retained for a **live** supply. A recoverable capability's
    // construction needs the same owners the startup path used, so the runtime keeps them
    // rather than making a recovery rebuild them.
    let persistence_for_dependencies = Arc::clone(&persistence);
    let settings_for_dependencies = Arc::clone(&settings);
    let release_for_supply = Arc::new(release.clone());
    let activity_index_for_dependencies = activity_index.clone();
    let sync_for_dependencies = sync.clone();
    let release_for_sidecar_facts = Arc::clone(&release_for_supply);
    let dependent = match dependent_p03 {
        Some(p03) => match open_dependent_services(
            options,
            release,
            &settings,
            &persistence,
            &broker,
            &broker_http,
            &activities,
            &agent_store,
            &sync,
            &activity_index,
            &activity_observer,
            Arc::clone(&p03),
            &config_degradation,
            max_concurrent_tool_calls,
        )
        .await
        {
            Ok(dependent) => Some(dependent),
            Err(error) => {
                // A real failure inside the dependent chain. Still not a Core
                // failure: the capability is refused with its reason recorded, and
                // control/status/stop/repair keep working.
                append_core_log(
                    "dependent_services_unavailable",
                    json!({
                        "capability": ConfigDependentCapability::ToolPolicy.key(),
                        "fault_code": ConfigDependentCapability::ToolPolicy.unavailable_code(),
                        "reason": error.to_string(),
                    }),
                );
                config_degradation.degrade(
                    ConfigDependentCapability::ToolPolicy,
                    None,
                    ConfigDependentCapability::ToolPolicy.unavailable_code(),
                    true,
                );
                None
            }
        },
        None => None,
    };
    append_startup_stage("agent_application_ready");
    // Record **what the startup path really did**, so a live
    // recovery can tell "already done, do not repeat" from "never reached, must be discharged".
    //
    // This is written after each service was really built, never before: a step marked on an
    // intention would be exactly the copyable marker the brief rejects. The two model steps are
    // recorded only when `P05Services::open` returned, because that constructor is what resumes the
    // journals and cancels the interrupted operations; the three Agent steps only when the
    // application was really created, because `new_for_process_startup` is what scans, restores and
    // backfills. The maintenance task is recorded when the task was really started.
    let startup_ledger = Arc::new(service_rebuild::StartupLedger::new());
    {
        use service_rebuild::OneTimeStartupStep as Step;
        if let Some(services) = dependent.as_ref() {
            if services.p05.is_some() {
                startup_ledger.mark_completed(Step::ModelConfigJournalReconciliation);
                startup_ledger.mark_completed(Step::InterruptedModelOperationCancellation);
            }
            if services.agent.is_some() {
                startup_ledger.mark_completed(Step::AgentInboxStartupScan);
                startup_ledger.mark_completed(Step::AgentWaitRestoration);
                startup_ledger.mark_completed(Step::AgentDerivedBackfill);
            }
            if services.p05_maintenance.is_some() {
                startup_ledger.mark_completed(Step::ModelMaintenanceTask);
            }
        }
    }
    // Destructured once, so each `Option` field is moved exactly once. The
    // dependent chain is either wholly present or wholly absent, which keeps
    // "the tool configuration is unusable" a single consistent fact rather than a
    // partially built service graph.
    let (
        agent_execution_for_state,
        commands,
        agent,
        p04,
        p05,
        security_sidecars,
        p05_maintenance,
        p07_maintenance,
    ) = match dependent {
        Some(services) => (
            services.agent_execution,
            services.commands,
            services.agent,
            services.p04,
            services.p05,
            Some(services.security_sidecars),
            services.p05_maintenance,
            Some(services.p07_maintenance),
        ),
        None => (None, None, None, None, None, None, None, None),
    };
    // One package owner per process, sharing the V2 writer. Startup recovery
    // (interrupted runs, scratch and import leftovers) happens exactly here.
    let packages = match package_services::PackageServices::open(
        broker.shared_store(),
        Arc::clone(&persistence),
        &paths.data_dir.join("v2-packages"),
    ) {
        Ok(packages) => {
            if let Err(error) = packages.collect() {
                append_core_log(
                    "package.startup_collect_failed",
                    json!({"reason": error.to_string()}),
                );
            }
            Some(Arc::new(packages))
        }
        Err(error) => {
            append_core_log("package.service_unavailable", json!({"reason": error.to_string()}));
            None
        }
    };
    let mcp = crate::mcp::McpRuntime::new(
        Arc::clone(&settings),
        Arc::clone(&broker),
        Some(crate::mcp::McpNetwork {
            gateway: Arc::clone(&broker_http),
            runtime: tokio::runtime::Handle::current(),
        }),
        Arc::new(crate::mcp::credentials::WindowsSecretStore::product()),
    );
    if let Some(execution) = &agent_execution_for_state {
        let _ = execution.mcp.set(Arc::clone(&mcp));
    }
    let skills = crate::skills::SkillRuntime::new(
        Arc::clone(&settings),
        p03_config.as_ref().map(|p03| Arc::clone(&p03.workspace)),
    );
    if let Some(execution) = &agent_execution_for_state {
        let _ = execution.skills.set(Arc::clone(&skills));
    }
    // An interrupted plugin change is finished or rolled back here, before anything a
    // plugin brings can be offered.
    let plugins = crate::plugins::PluginRuntime::new(Arc::clone(&settings), Some(Arc::clone(&mcp)));
    mcp.attach_plugins(&plugins);
    skills.attach_plugins(Arc::clone(&plugins));
    // The first listing hashes every Skill file; a thread
    // at low CPU priority does it once now, so the page finds them known.
    crate::skills::warm_in_background(Arc::clone(&skills));
    if let Some(execution) = &agent_execution_for_state {
        let _ = execution.plugins.set(Arc::clone(&plugins));
    }
    let mut services = RuntimeServices {
        packages,
        broker,
        activity_index,
        activity_observer: activity_observer.clone(),
        activity_runtime_instance_id: options.runtime_instance_id,
        commands: Arc::new(match commands {
            Some(commands) => DependentSlot::filled(commands),
            None => DependentSlot::empty(),
        }),
        sync,
        broker_http,
        agent_execution: Arc::new(match agent_execution_for_state {
            Some(execution) => DependentSlot::filled(execution),
            None => DependentSlot::empty(),
        }),
        agent: Arc::new(match agent {
            Some(agent) => DependentSlot::filled(agent),
            None => DependentSlot::empty(),
        }),
        p04_slot: Arc::new(match p04 {
            Some(p04) => DependentSlot::filled(Arc::new(p04)),
            None => DependentSlot::empty(),
        }),
        p05_slot: Arc::new(match p05 {
            Some(p05) => DependentSlot::filled(Arc::new(p05)),
            None => DependentSlot::empty(),
        }),
        p03_slot: Arc::new(match p03_config {
            Some(p03) => DependentSlot::filled(p03),
            None => DependentSlot::empty(),
        }),
        agent_store: Arc::clone(&agent_store),

        _config_watcher: config_watcher,
        _p05_maintenance: p05_maintenance,
        _p07_maintenance: p07_maintenance,
        _settings: settings,
        mcp,
        skills,
        plugins,
        config_degradation,
        config_recovery,
        startup_ledger: Arc::clone(&startup_ledger),
        persistence,
        p03_dependencies: Arc::new(P03Dependencies {
            persistence: Arc::clone(&persistence_for_dependencies),
            settings: Arc::clone(&settings_for_dependencies),
            activity_index: activity_index_for_dependencies,
            activity_observer: activity_observer.clone(),
            runtime_instance_id: options.runtime_instance_id,
            sync: sync_for_dependencies,
            sidecar_facts: Some(SidecarFacts {
                options: options.clone(),
                release: Arc::clone(&release_for_sidecar_facts),
            }),
            max_concurrent_tool_calls,
            runtime_handle: tokio::runtime::Handle::try_current().ok(),
        }),
        release: release_for_supply,
        activities: Arc::new(activities_for_services),
        security_sidecars: Arc::new(std::sync::Mutex::new(security_sidecars.map(Arc::new))),
    };
    // Attach the live rebuilder **after** the services exist,
    // because it needs their slot handles and the live owners. Without this a configuration
    // recovery could only clear records, and a capability whose slot is still vacant would
    // be reported available on the strength of a deleted marker.
    attach_dependent_rebuilder(&mut services, Arc::clone(&startup_ledger));
    // Finish a conversation deletion or a Workspace removal an earlier run did not.
    crate::session_lifecycle::Purger::of(&services).spawn_resume();
    Ok(services)
}
