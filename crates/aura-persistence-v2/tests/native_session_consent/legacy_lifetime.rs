//! Existing behavior tests extracted to keep the parent within its size ceiling.
use super::*;

#[test]
fn a_later_permission_revision_invalidates_the_consent_even_after_returning_to_full_access() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let issued = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    legacy::legacy(&mut fixture, issued);
    let second = fixture.operation("shell.powershell", "Get-Process");
    let (_, decision) = fixture.derive(&second).expect("offered");
    let backend = fixture.native_backend();
    // The revision in force is newer than the one the consent was approved under:
    // this is the downgrade-then-upgrade case.
    let refused = fixture.reserve(
        &second,
        &backend,
        &decision,
        fixture.snapshot.session_permission_revision + 1,
    );
    assert!(refused.is_err(), "a permission change ends the consent: {refused:?}");
    // The same Operation reserves once the revision matches again, which proves the
    // refusal above was the revision and not an unrelated failure. Reusing the
    // Operation is deliberate: a refused reservation must have left no Attempt.
    let (_, retry_decision) = fixture.derive(&second).expect("still offered");
    let retry = fixture.reserve(
        &second,
        &backend,
        &retry_decision,
        fixture.snapshot.session_permission_revision,
    );
    assert!(
        retry.is_ok(),
        "with the approved revision in force the same consent reserves: {retry:?}"
    );
}

#[test]
fn a_consent_from_another_core_runtime_instance_is_not_offered() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    // Approved by a previous Core run: same data root, different instance.
    let previous = RuntimeInstanceId::new();
    let issued = fixture.approve_with_runtime(&first, ApprovalDecisionKind::AllowSession, previous);
    let grants = legacy::legacy(&mut fixture, issued);
    let grant = grants[0].clone();
    let second = fixture.operation("shell.powershell", "Get-Process");
    let (_, decision) = fixture.derive(&second).expect("the Grant is still offered by the store");
    let backend = fixture.native_backend();
    assert!(
        fixture
            .reserve(&second, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_err(),
        "a restarted Core does not inherit a consent"
    );
    // And the management projection agrees with the dispatch path instead of
    // contradicting it — including about *how* it stopped applying. A consent from an
    // earlier Core run is not waiting to take effect; the restart already ended it.
    let view = fixture.consent_view(grant.grant_id).expect("the record still projects");
    assert!(
        !view.availability.is_in_force(),
        "a consent issued by an earlier Core run is not in force"
    );
    assert_eq!(
        view.availability,
        NativeSessionConsentAvailability::Superseded,
        "a restarted Core supersedes the consent rather than leaving it pending"
    );
}
