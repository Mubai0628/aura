use super::*;

/// The management match must compare the bindings, not only the identifiers.
///
/// A consent row and a Grant that agree about `grant_id` and the revision can still
/// describe two different authorizations. Only the two revisions may legitimately move;
/// a row whose principal, runtime, permission revision, Shell range, approval or approved
/// Operation was replaced is not this consent, and the production view must stop showing
/// it — the Store's own reservation already refuses that pair.
#[test]
fn matching_a_consent_requires_every_immutable_binding() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.cmd", "echo approved");
    let grant = fixture.approve(&first, ApprovalDecisionKind::AllowSession).remove(0);
    let consent =
        security::native_session_consent(fixture.store.database.connection(), grant.grant_id)
            .expect("consent")
            .expect("present");
    assert!(consent.matches_grant(&grant), "the row the Store just wrote describes its Grant");

    let mut rebound = grant.clone();
    let reference = rebound.scope.consent.as_mut().expect("the Grant carries the consent");
    reference.approved.principal_id = PrincipalId::new();
    assert!(
        !consent.matches_grant(&rebound),
        "a different authorized principal is not this consent"
    );
    assert!(fixture.consent_row_view(&consent, &rebound).is_none());

    let base = grant.scope.consent.clone().expect("the Grant carries the consent");
    let mut variants: Vec<(&str, NativeSessionConsentRef)> = Vec::new();
    let mut variant = base.clone();
    variant.core_runtime_instance_id = RuntimeInstanceId::new();
    variants.push(("a different Core runtime instance", variant));
    let mut variant = base.clone();
    variant.session_permission_revision += 1;
    variants.push(("a different session permission revision", variant));
    let mut variant = base.clone();
    variant.approval_id = aura_domain_common::ApprovalRequestId::new();
    variants.push(("a different approval", variant));
    let mut variant = base.clone();
    variant.approved_operation_fingerprint =
        OperationFingerprint(CanonicalSha256HashV1::sha256(b"another operation"));
    variants.push(("a different approved Operation", variant));
    let mut variant = base.clone();
    variant.approved.shells = std::iter::once(NativeShell::Cmd).collect();
    variants.push(("a narrowed Shell range", variant));
    let mut variant = base.clone();
    variant.approved.approved_shell = NativeShell::PowerShell;
    variants.push(("a different audit Shell", variant));
    for (label, reference) in variants {
        let mut changed = grant.clone();
        changed.scope.consent = Some(reference);
        assert!(!consent.matches_grant(&changed), "{label} must not be accepted as this consent");
        assert!(
            fixture.consent_row_view(&consent, &changed).is_none(),
            "{label} must not be projected as usable"
        );
    }

    // The revision a withdrawal records is the one legitimate difference, and it keeps
    // matching so a withdrawn consent still projects as revoked.
    let mut withdrawn = grant.clone();
    withdrawn.revision = Revision::new(2);
    withdrawn.revoked_at = Some(fixture.at);
    let mut row = consent.clone();
    row.revoked_grant_revision = Some(Revision::new(2));
    withdrawn.scope.consent = Some(row.reference());
    assert!(row.matches_grant(&withdrawn), "a withdrawal is not a binding change");

    let mut rebound = grant.clone();
    rebound.revision = Revision::new(2);
    rebound.scope.consent.as_mut().expect("consent").grant_revision = Revision::new(2);
    assert!(
        !consent.matches_grant(&rebound),
        "the Grant and its reference cannot replace the stored issuance revision"
    );
    assert!(fixture.consent_row_view(&consent, &rebound).is_none());
    withdrawn.scope.consent.as_mut().expect("consent").grant_revision = Revision::new(2);
    assert!(!row.matches_grant(&withdrawn), "withdrawal does not change the issuance revision");
}
