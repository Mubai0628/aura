use crate::*;
use aura_core_base::{blocking_pool, sync_boundary};

/// What one configuration-maintenance storage section really did.
///
/// WHY THIS IS TYPED AND NOT A `bool`. "The section returned" and "the change is reflected" are
/// different statements, and the consumer has to act differently on each of them: a refusal
/// proves the work did not run and the change is still pending, an unknown outcome means the work
/// started and produced no answer, and a real failure is a reported failure rather than a silent
/// success. Collapsing them into one boolean is exactly how an already-consumed configuration
/// change gets dropped.
#[derive(Debug)]
pub(crate) enum ConfigMaintenanceOutcome {
    /// The real work ran and the consumed change is reflected in the projection.
    Applied,
    /// The section was refused: it provably did not run, so the change is still pending.
    NotRun,
    /// The section started and produced no value at all (a panic). It may have had a partial
    /// effect and it is certainly not a success.
    Unknown,
    /// The real work ran and failed. The error is reported as itself.
    Failed(String),
}

/// Runs one Models configuration-maintenance rebuild as one bounded section.
///
/// The async consumer awaits the *real* typed result through `run_owned`, which holds no boundary
/// permit while it waits, so a section that outlives the caller's budget is still settled into
/// this consumer instead of being abandoned - and a budget expiring can therefore never become
/// the reason for a second commit.
pub(crate) async fn run_model_projection_maintenance(
    sync: &sync_boundary::SyncBoundary,
    p05: P05Services,
) -> ConfigMaintenanceOutcome {
    let admission = sync
        .run_owned(
            sync_boundary::SyncSection::Projection,
            sync_boundary::SyncSubject::new().method("config.models.rebuild"),
            move || p05.rebuild_model_projections_if_stale().map(|_| ()),
        )
        .await;
    match admission {
        blocking_pool::BlockingAdmission::Completed(Ok(())) => ConfigMaintenanceOutcome::Applied,
        blocking_pool::BlockingAdmission::Completed(Err(error)) => {
            ConfigMaintenanceOutcome::Failed(format!("{error:#}"))
        }
        blocking_pool::BlockingAdmission::Refused { waited_ms } => {
            append_core_log(
                "models_projection_maintenance_not_run",
                json!({ "reason": "boundary_refused", "waited_ms": waited_ms }),
            );
            ConfigMaintenanceOutcome::NotRun
        }
        blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
            append_core_log(
                "models_projection_maintenance_unknown",
                json!({ "reason": "section_produced_no_value", "waited_ms": waited_ms }),
            );
            ConfigMaintenanceOutcome::Unknown
        }
    }
}

/// Runs one Tool Settings maintenance publication as one bounded section.
///
/// Same contract as [`run_model_projection_maintenance`]: the async consumer awaits the real
/// typed result, the configuration revision and the publication semantics are the existing ones,
/// and the publication error the control service already reports stays a separate field rather
/// than being folded into "it worked".
pub(crate) async fn run_tool_settings_maintenance(
    sync: &sync_boundary::SyncBoundary,
    settings: Arc<SettingsFacade>,
    persistence: Arc<PersistenceCoordinator>,
    configuration_rejected: bool,
) -> ConfigMaintenanceOutcome {
    let admission = sync
        .run_owned(
            sync_boundary::SyncSection::Projection,
            sync_boundary::SyncSubject::new().method("config.tools.publish"),
            move || publish_current_tool_settings(&settings, &persistence, configuration_rejected),
        )
        .await;
    match admission {
        blocking_pool::BlockingAdmission::Completed(Ok(())) => ConfigMaintenanceOutcome::Applied,
        blocking_pool::BlockingAdmission::Completed(Err(error)) => {
            ConfigMaintenanceOutcome::Failed(format!("{error:#}"))
        }
        blocking_pool::BlockingAdmission::Refused { waited_ms } => {
            append_core_log(
                "tool_settings_maintenance_not_run",
                json!({ "reason": "boundary_refused", "waited_ms": waited_ms }),
            );
            ConfigMaintenanceOutcome::NotRun
        }
        blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
            append_core_log(
                "tool_settings_maintenance_unknown",
                json!({ "reason": "section_produced_no_value", "waited_ms": waited_ms }),
            );
            ConfigMaintenanceOutcome::Unknown
        }
    }
}

pub(crate) fn spawn_p05_maintenance(
    settings: &SettingsFacade,
    p05: P05Services,
    sync: sync_boundary::SyncBoundary,
) -> Result<RuntimeBackgroundTask> {
    let config_events = settings
        .subscribe()
        .context("failed to subscribe P05 projection maintenance to config changes")?;
    let task = tokio::spawn(async move {
        let mut poll = time::interval(Duration::from_millis(250));
        let mut next_catalog_check = Instant::now() + Duration::from_secs(60);
        // A consumed configuration change this consumer has not reflected yet. It is re-checked on
        // the 250 ms poll the task already had - never on a timer of its own - so a full boundary
        // costs one refused section per tick and the change is reflected as soon as one is
        // admitted. The section reads the *current* last-known-good settings when it runs, so a
        // change that arrives while an earlier one is still pending is reflected as the newest
        // state rather than as a stale one.
        let mut model_change_pending = false;
        loop {
            poll.tick().await;
            loop {
                match config_events.try_recv() {
                    Ok(event)
                        if event.module == ConfigModule::Models
                            && matches!(
                                event.kind,
                                ConfigEventKind::Changed | ConfigEventKind::Recovered
                            ) =>
                    {
                        model_change_pending = true;
                    }
                    Ok(_) => {}
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                }
            }
            if model_change_pending {
                match run_model_projection_maintenance(&sync, p05.clone()).await {
                    ConfigMaintenanceOutcome::Applied => model_change_pending = false,
                    // The section provably did not run, so the changed configuration is still
                    // this consumer's responsibility: the next tick tries again.
                    ConfigMaintenanceOutcome::NotRun => {}
                    // It may have had a partial effect, so the change stays pending and is
                    // re-derived from the current settings rather than assumed applied.
                    ConfigMaintenanceOutcome::Unknown => {}
                    // A real failure is reported as itself and is not retried in a loop; the
                    // pre-existing behaviour for a failed rebuild is to report and move on, and a
                    // later configuration change re-drives it.
                    ConfigMaintenanceOutcome::Failed(error) => {
                        model_change_pending = false;
                        eprintln!("P05 model projection rebuild failed: {error}");
                    }
                }
            }
            if Instant::now() >= next_catalog_check {
                if let Err(error) = p05.refresh_catalog_if_due().await {
                    eprintln!("P05 scheduled provider-catalog refresh failed: {error:#}");
                }
                next_catalog_check = Instant::now() + Duration::from_secs(60);
            }
        }
    });
    Ok(RuntimeBackgroundTask(task))
}

pub(crate) fn spawn_p07_maintenance(
    settings: &Arc<SettingsFacade>,
    persistence: Arc<PersistenceCoordinator>,
    sync: sync_boundary::SyncBoundary,
) -> Result<RuntimeBackgroundTask> {
    let config_events = settings
        .subscribe()
        .context("failed to subscribe P07 Tool Settings projection maintenance")?;
    let settings = Arc::clone(settings);
    let startup_recovered = settings.startup_events().iter().any(|event| {
        event.module == ConfigModule::Tools && event.kind == ConfigEventKind::Recovered
    });
    // STARTUP IS DELIBERATELY LEFT SYNCHRONOUS. This call is reached from
    // `initialize_services_at`, which `run_core` awaits before the Named Pipe is even listening:
    // there is no live control lane, heartbeat or request for a parked reactor worker to starve,
    // so moving startup onto the boundary would add a refusal path to work that must happen
    // exactly once and would not make any running consumer more responsive. The *event callback*
    // below is the path that runs while the Core is serving, and that one is a bounded section.
    publish_current_tool_settings(&settings, &persistence, startup_recovered)
        .context("failed to initialize P07 Tool Settings projection")?;
    let list_changes = persistence
        .commit_current_tool_global_list_projections()
        .context("failed to initialize P07 Tool Package/Draft list projections")?;
    if let Err(error) = persistence.publish_tool_projection_changes(&list_changes) {
        append_core_log(
            "tool_global_list_projection_publish_failed",
            json!({ "reason": error.to_string() }),
        );
    }
    let task = tokio::spawn(async move {
        let mut poll = time::interval(Duration::from_millis(250));
        // The same bounded pending flag as the Models consumer, and the same rule: the last
        // drained event decides the publication state, so the projection always reflects the
        // newest last-known-good configuration and can never flow backwards.
        let mut tool_change_pending = false;
        let mut configuration_rejected = false;
        loop {
            poll.tick().await;
            loop {
                let event = match config_events.try_recv() {
                    Ok(event) => event,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                };
                if event.module != ConfigModule::Tools {
                    continue;
                }
                tool_change_pending = true;
                configuration_rejected =
                    matches!(event.kind, ConfigEventKind::Rejected | ConfigEventKind::Recovered);
            }
            if tool_change_pending {
                let rejected = configuration_rejected;
                match run_tool_settings_maintenance(
                    &sync,
                    Arc::clone(&settings),
                    Arc::clone(&persistence),
                    rejected,
                )
                .await
                {
                    ConfigMaintenanceOutcome::Applied => tool_change_pending = false,
                    ConfigMaintenanceOutcome::NotRun => {}
                    ConfigMaintenanceOutcome::Unknown => {}
                    ConfigMaintenanceOutcome::Failed(error) => {
                        tool_change_pending = false;
                        append_core_log(
                            "tool_settings_projection_failed",
                            json!({ "reason": error }),
                        );
                    }
                }
            }
        }
    });
    Ok(RuntimeBackgroundTask(task))
}

pub(crate) fn publish_current_tool_settings(
    settings: &SettingsFacade,
    persistence: &PersistenceCoordinator,
    configuration_rejected: bool,
) -> Result<()> {
    let snapshot = settings
        .tool_settings_snapshot()
        .context("failed to read last-known-good Tool Settings")?;
    let published = p07_registry::ToolRegistryControlServiceV1
        .publish_settings(persistence, snapshot, configuration_rejected)
        .context("failed to commit Tool Settings projection")?;
    if let Some(error) = published.projection_publish_error {
        append_core_log(
            "tool_settings_projection_publish_failed",
            json!({ "reason": error.to_string() }),
        );
    }
    Ok(())
}
