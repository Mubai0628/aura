//! Existing behavior tests extracted to keep the parent within its size ceiling.
use super::*;

#[test]
fn native_is_explicit_once_or_session_even_in_full_access() {
    let mut c = Case::new(true);
    c.backend.class = BackendClass::Native;
    c.backend.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    c.profile = SecurityProfile::FullAccess;
    c.snapshot.mode = PermissionMode::FullAccess;
    assert!(selector::select(&c.input(), &[c.backend.clone()], None, None, false).is_err());
    let selected =
        selector::select(&c.input(), &[c.backend.clone()], Some(&c.backend.id), None, false)
            .expect("explicit preview");
    let PolicyAuthorization::Ask { proposal } = &selected[0].authorization else {
        panic!("Native must ask")
    };
    assert!(proposal.reasons.iter().any(|reason| reason == "backend.native_explicit"));
    assert!(proposal.reasons.iter().any(|reason| reason == "execution.isolation_reduced"));
    assert_eq!(
        proposal.allowed_decisions,
        vec![
            ApprovalDecisionKind::AllowOnce,
            ApprovalDecisionKind::AllowSession,
            ApprovalDecisionKind::Deny
        ]
    );
    c.approve(AuthorizationLifetime::Workspace { workspace_id: c.snapshot.workspace_id });
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. }));
    c.grants[0].scope.lifetime =
        AuthorizationLifetime::Session { session_id: c.snapshot.session_id };
    c.allow();
    c.profile = SecurityProfile::Strict;
    assert!(
        selector::select(&c.input(), &[c.backend.clone()], Some(&c.backend.id), None, false)
            .is_err()
    );
}
