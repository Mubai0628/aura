use crate::*;

pub(crate) async fn initialize_runtime_services(
    options: &CoreOptions,
    release: &CoreReleaseContext,
) -> StartupMode {
    if options.test_mode == CoreTestMode::MigrationRecoveryRequired {
        return StartupMode::RecoveryRequired {
            reason: "injected migration recovery requirement".to_owned(),
        };
    }
    classify_startup(try_initialize_runtime_services(options, release).await)
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

pub(crate) async fn try_initialize_runtime_services(
    options: &CoreOptions,
    release: &CoreReleaseContext,
) -> Result<RuntimeServices> {
    let paths =
        AuraPaths::from_environment_for_namespace(&release.release_descriptor.bundle_namespace)
            .context("failed to resolve Aura data paths")?;
    initialize_services_at(options, release, paths).await
}
