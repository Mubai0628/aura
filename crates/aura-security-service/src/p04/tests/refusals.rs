use super::*;
use aura_core_base::command_settlement;

#[test]
fn persistence_refusal_and_unsettled_work_remain_distinct() {
    let correlation = CorrelationId::new();
    let busy = persistence_error(PersistenceError::WriterBusy { class: "sync" }, correlation);
    assert_eq!(busy.code.as_str(), "persistence.busy");
    assert!(busy.retryable);
    assert!(!command_settlement::envelope_is_unconfirmed(&busy));
    let unknown =
        persistence_error(PersistenceError::WriterUnresponsive { class: "sync" }, correlation);
    assert_eq!(unknown.code.as_str(), "persistence.outcome_unknown");
    assert!(!unknown.retryable);
    assert!(command_settlement::envelope_is_unconfirmed(&unknown));
    let internal =
        persistence_error(PersistenceError::InvalidRequest("fixture".into()), correlation);
    assert_eq!(internal.code.as_str(), "persistence.internal");
}

#[test]
fn failed_elevated_cleanup_maps_to_state_unknown_before_apply() {
    let error = anyhow::Error::new(ElevatedHelperLaunchError::CleanupFailedStateUnknown {
        verification_error: std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "verification fixture",
        ),
        cleanup_error: std::io::Error::other("cleanup fixture"),
    });
    assert_eq!(
        host_preparation_execution_error_code(
            HostPreparationOperationState::AwaitingElevation,
            &error,
        ),
        "sandbox.elevated_state_unknown"
    );

    let ordinary = anyhow::Error::new(ElevatedHelperLaunchError::Platform(std::io::Error::new(
        std::io::ErrorKind::PermissionDenied,
        "UAC cancelled",
    )));
    assert_eq!(
        host_preparation_execution_error_code(
            HostPreparationOperationState::AwaitingElevation,
            &ordinary,
        ),
        "sandbox.host_preparation_uac_failed"
    );
}

#[test]
fn release_trust_failure_is_reported_before_uac() {
    let error = anyhow::Error::new(ActiveReleaseTrustError::new(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "fixture Release Root is unavailable",
    )));
    assert_eq!(
        host_preparation_execution_error_code(
            HostPreparationOperationState::AwaitingElevation,
            &error,
        ),
        "sandbox.host_preparation_release_trust_failed"
    );
}

#[test]
fn a_pre_write_refusal_is_not_reported_as_an_unknown_state() {
    // The refusal proves nothing was written, so it must not be closed out as
    // "we do not know what happened" — the state the operation was in when the
    // helper answered does not change that.
    let refusal = anyhow::Error::new(crate::p04_broker::ElevatedHelperRefusal::new(
        "sandbox.host_preparation_capability_conflict",
    ));
    assert_eq!(
        host_preparation_execution_error_code(HostPreparationOperationState::Applying, &refusal,),
        "sandbox.host_preparation_capability_conflict"
    );
    assert_eq!(
        host_preparation_failure_category("sandbox.host_preparation_capability_conflict"),
        ErrorCategory::Conflict
    );
    assert!(matches!(
        host_preparation_execution_error_code(
            HostPreparationOperationState::Applying,
            &anyhow::Error::new(std::io::Error::other("the pipe closed")),
        ),
        "sandbox.elevated_state_unknown"
    ));
}

#[test]
fn the_pre_write_refusal_set_is_closed() {
    // A helper must not be able to turn a real uncertainty into a definite
    // refusal by sending an unknown code.
    assert_eq!(
        aura_permission_domain::host_preparation_pre_write_refusal_code(
            "sandbox.host_preparation_capability_conflict"
        ),
        Some("sandbox.host_preparation_capability_conflict")
    );
    assert_eq!(
        aura_permission_domain::host_preparation_pre_write_refusal_code(
            "sandbox.elevated_state_unknown"
        ),
        None
    );
    assert_eq!(
        aura_permission_domain::host_preparation_pre_write_refusal_code("anything.else"),
        None
    );
}
