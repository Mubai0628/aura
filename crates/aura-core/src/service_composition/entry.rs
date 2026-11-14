use crate::*;

pub(crate) async fn initialize_runtime_services(
    options: &CoreOptions,
    release: &CoreReleaseContext,
    paths: AuraPaths,
) -> StartupMode {
    if options.test_mode == CoreTestMode::MigrationRecoveryRequired {
        return StartupMode::RecoveryRequired {
            reason: "injected migration recovery requirement".to_owned(),
        };
    }
    let mode = classify_startup(initialize_services_at(options, release, paths).await);
    // The stores opened where the data is now: the copy a move left can go.
    if matches!(mode, StartupMode::Ready(_)) {
        aura_core_base::locations::remove_old_copies_later();
    }
    mode
}

pub(crate) fn classify_startup(result: Result<RuntimeServices>) -> StartupMode {
    match result {
        Ok(services) => StartupMode::Ready(Box::new(services)),
        Err(error) => {
            let reason = error.to_string();
            append_core_log(
                "core_runtime_initialization_failed",
                json!({ "reason": reason, "error_chain": format!("{error:#}") }),
            );
            StartupMode::RecoveryRequired { reason }
        }
    }
}
