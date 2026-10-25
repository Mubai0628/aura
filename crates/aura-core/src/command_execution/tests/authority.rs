use super::*;

#[test]
fn mode_change_after_reservation_prevents_launch_registration() {
    let case = Case::new();
    case.port
        .persistence
        .set_session_permission_mode(
            case.request.operation.content.identity.session_id.unwrap(),
            1,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .unwrap();
    let mut registered = false;
    assert!(
        case.port
            .claim(&case.request, &mut || {
                registered = true;
                Ok(())
            })
            .is_err()
    );
    assert!(!registered);
    assert!(matches!(
        case.port.reconcile(case.request.binding.attempt.clone()).unwrap(),
        ReconcileResult::Unknown { .. }
    ));
}

#[test]
fn native_completion_records_isolation_reduced() {
    let mut case = Case::new();
    case.port.claim(&case.request, &mut || Ok(())).unwrap();
    case.request.plan.prepared.backend.class = BackendClass::Native;
    case.port.complete(&case.request, None, true, true, None, None).unwrap();
    let ReconcileResult::Completed { result } =
        case.port.reconcile(case.request.binding.attempt.clone()).unwrap()
    else {
        panic!("stored result")
    };
    assert!(result.warnings.iter().any(|warning| warning == "execution.isolation_reduced"));
    assert_eq!(
        "execution.isolation_reduced"
            .parse::<NormalizedIssueCode>()
            .expect("Native isolation warning is a registered model-visible code")
            .as_str(),
        "execution.isolation_reduced"
    );
}

#[test]
fn expired_reserved_authority_is_not_a_launch_ticket() {
    let case = Case::new();
    let future = UtcTimestamp::from_value(UtcTimestamp::now().value() + time::Duration::hours(1));
    assert!(
        aura_persistence_v2::execution::dispatch_authority(
            case.port.journal.lock().unwrap().database.connection(),
            &case.request,
            future
        )
        .is_err()
    );
}
