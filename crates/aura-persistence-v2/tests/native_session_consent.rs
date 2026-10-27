//! The reusable Native session consent, end to end at the Store boundary.
//!
//! These cases drive the real `security`, `security_approval` and `execution`
//! functions on a real SQLite V2 store. They deliberately do not re-run the same
//! Operation with a different lifetime: the whole point of the consent is that a
//! *different* Operation — a different script, a different correlation, a different
//! permission snapshot, and in the owner's own words "different Shell" — is covered,
//! so every reuse case below is a new Operation with its own fingerprint.
//!
//! Time is injected rather than slept through. The confirmation window and the
//! consent's own bounds are different things, and the only honest way to assert the
//! difference is to move the clock.
#![allow(clippy::unwrap_used)]

#[path = "native_session_consent/legacy_lifetime.rs"]
mod legacy_lifetime;
#[path = "native_session_consent/legacy_shape.rs"]
mod legacy_shape;

#[path = "native_session_consent/binding_integrity.rs"]
mod binding_integrity;
#[path = "native_session_consent/legacy.rs"]
mod legacy;

use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    AgentRunId, CorrelationId, PermissionGrantId, PermissionSnapshotId, PrincipalId, Revision,
    RuntimeInstanceId, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_execution_contract::{
    AttemptRef, BackendDescriptor, BackendId, BackendReadiness, CancellationState, DispatchBinding,
    DispatchRequest, ExecutionAttempt, ExecutionCompatibility, ExecutionPlan, OperationNetwork,
    PreparedExecution, SecuredOperation,
};
use aura_permission_domain::{
    NativeSessionConsentAvailability, NativeSessionConsentScopeView, NativeSessionConsentView,
};
use aura_persistence_v2::{V2Store, security, security_approval, workspace};
use aura_security_contract::*;

#[path = "native_session_consent/fixture.rs"]
mod fixture;
use fixture::*;
#[path = "native_session_consent/editor_publication.rs"]
mod editor_publication;
#[path = "native_session_consent/file_session.rs"]
mod file_session;
#[path = "native_session_consent/network_session.rs"]
mod network_session;
#[path = "native_session_consent/s08_lifetime.rs"]
mod s08_lifetime;

#[test]
fn a_session_approval_of_a_builtin_shell_mints_one_reusable_consent() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    assert_eq!(grants.len(), 1);
    let grant = &grants[0];
    let consent = grant.scope.consent.as_ref().expect("reusable consent");
    assert_eq!(consent.grant_id, grant.grant_id);
    assert_eq!(consent.approved.approved_shell, NativeShell::PowerShell);
    assert!(
        consent.approved.shells.contains(&NativeShell::PowerShell)
            && consent.approved.shells.contains(&NativeShell::Cmd),
        "one session answer covers ordinary PowerShell and CMD commands"
    );
    assert_eq!(consent.session_permission_revision, fixture.snapshot.session_permission_revision);
    assert_eq!(consent.core_runtime_instance_id, fixture.runtime);
    assert_eq!(
        consent.approved_operation_fingerprint, first.fingerprint,
        "the approved Operation stays recorded for audit"
    );
    assert_eq!(
        grant.scope.operation_fingerprint, first.fingerprint,
        "the stored Grant keeps the approved fingerprint, not a later one"
    );
    let listed =
        security::native_session_consents(fixture.store.database.connection(), fixture.session)
            .expect("consents");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        security::native_session_consent(fixture.store.database.connection(), grant.grant_id)
            .expect("consent"),
        Some(listed[0].clone())
    );
}

#[test]
fn a_session_consent_does_not_carry_the_confirmation_window() {
    // The confirmation window bounds how long the user has to answer. The consent
    // bounds this Core run. Attaching the window to the consent made an approval
    // taken with one minute left valid for one minute.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let grants = fixture.approve_at(&first, ApprovalDecisionKind::AllowSession, 1);
    assert!(
        grants[0].expires_at.is_none(),
        "a reusable consent is bounded by its runtime, permissions and withdrawal, not by a clock"
    );
}

#[test]
fn a_consent_still_derives_and_reserves_long_after_the_confirmation_window() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let backend = fixture.native_backend();
    // Well past the ten-minute window the approval was taken with, and past the five
    // minute dispatch credential the first call would have used.
    fixture.advance_minutes(11);
    let second = fixture.operation("shell.powershell", "Get-Process");
    let (_, decision) = fixture.derive(&second).expect("still a live consent");
    assert!(
        fixture
            .reserve(&second, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_ok(),
        "a consent from this run is not expired by the window that produced it"
    );
    fixture.advance_minutes(19);
    let third = fixture.operation("shell.cmd", "dir");
    let (_, decision) = fixture.derive(&third).expect("still a live consent at +30 minutes");
    assert!(
        fixture
            .reserve(&third, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_ok(),
        "thirty minutes later, in the same Core run, the consent still reserves"
    );
}

#[test]
fn a_dispatch_credential_still_expires_on_its_own() {
    // The separation runs both ways: the consent has no clock, and the short-lived
    // decision still does. A caller that tries to spend an expired decision is
    // refused even though the consent behind it is live.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let second = fixture.operation("shell.powershell", "Get-Process");
    let (_, decision) = fixture.derive(&second).expect("offered");
    let backend = fixture.native_backend();
    fixture.advance_minutes(6);
    assert!(
        fixture
            .reserve(&second, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_err(),
        "the decision's own expiry still applies"
    );
}

#[test]
fn an_approval_at_the_last_moment_of_the_window_still_authorizes_its_dispatch() {
    // The reported defect: with the window nearly over, the fixed five-minute
    // dispatch credential overran the Grant's deadline and the caller was asked
    // again. The consent has no such deadline, so the dispatch it was approved for
    // runs.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    // Nine minutes fifty-nine seconds into a ten-minute window.
    fixture.advance_minutes(9);
    fixture.at = UtcTimestamp::from_value(fixture.at.value() + time::Duration::seconds(59));
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    assert_eq!(grants.len(), 1);
    let (_, decision) = fixture.derive(&first).expect("a just-approved consent derives");
    let backend = fixture.native_backend();
    assert!(
        fixture
            .reserve(&first, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_ok(),
        "an approval taken at the edge of the window still authorizes the call it was for"
    );
}

#[test]
fn an_approval_after_the_window_closed_is_refused() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let requested_at = fixture.at;
    let expires_at = UtcTimestamp::from_value(requested_at.value() + time::Duration::minutes(10));
    let approval = AuthorizationApproval {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: AuthorizationScope {
            operation_fingerprint: first.fingerprint.clone(),
            profile: SecurityProfile::FullAccess,
            backend_requirement: BackendRequirement {
                allowed_classes: [BackendClass::Native].into(),
                guarantees: Default::default(),
            },
            lifetime: AuthorizationLifetime::Once { operation_id: first.operation_id },
            consent: None,
            network_consent: None,
            capability_limit: None,
            memory_consent: None,
            mcp_consent: None,
            chrome_consent: None,
            file_consent: None,
        },
        allowed_decisions: vec![
            ApprovalDecisionKind::AllowOnce,
            ApprovalDecisionKind::AllowSession,
        ],
        state: ApprovalState::Pending,
        requested_at,
        expires_at,
    };
    fixture.advance_minutes(11);
    let at = fixture.at;
    let revision = fixture.snapshot.session_permission_revision;
    let runtime = fixture.runtime;
    let refused = fixture.store.database.unit_of_work(|tx| {
        security::insert_approval(tx, &approval)?;
        security_approval::resolve(
            tx,
            approval.approval_id,
            Revision::INITIAL,
            &first,
            ApprovalDecisionKind::AllowSession,
            BackendClass::Native,
            revision,
            runtime,
            at,
        )
    });
    assert!(refused.is_err(), "an expired confirmation window cannot be answered: {refused:?}");
    assert!(
        security::native_session_consents(fixture.store.database.connection(), fixture.session)
            .expect("consents")
            .is_empty(),
        "a refused resolution mints nothing"
    );
}

#[test]
fn allow_once_never_mints_a_consent_and_the_next_call_still_has_none() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowOnce);
    assert_eq!(grants.len(), 1);
    assert!(grants[0].scope.consent.is_none());
    assert!(
        security::native_session_consents(fixture.store.database.connection(), fixture.session)
            .expect("consents")
            .is_empty()
    );
    let second = fixture.operation("shell.powershell", "Get-Process");
    assert!(
        fixture.derive(&second).is_none(),
        "a new call finds no reusable consent after a once-only approval"
    );
}

#[test]
fn one_approval_covers_a_different_script_a_new_snapshot_and_the_other_shell() {
    // The owner's own sequence: approve PowerShell once, then run a different script,
    // then CMD, then PowerShell again. Each is a new Operation with its own
    // correlation, snapshot and fingerprint, and none of them asks again.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let backend = fixture.native_backend();
    let mut seen = Vec::new();
    for (shell, script) in [
        ("shell.powershell", "Get-ChildItem"),
        ("shell.cmd", "dir"),
        ("shell.powershell", "Get-Process"),
    ] {
        let next = fixture.operation(shell, script);
        assert!(!seen.contains(&next.fingerprint), "each call is a distinct Operation fingerprint");
        seen.push(next.fingerprint.clone());
        let (grant, decision) =
            fixture.derive(&next).unwrap_or_else(|| panic!("{shell} should reuse the consent"));
        assert_eq!(
            decision.scope.operation_fingerprint, next.fingerprint,
            "the decision names the Operation being dispatched"
        );
        assert_eq!(
            grant.scope.operation_fingerprint, first.fingerprint,
            "the stored Grant still names the Operation the user approved"
        );
        let reserved = fixture.reserve(
            &next,
            &backend,
            &decision,
            fixture.snapshot.session_permission_revision,
        );
        assert!(reserved.is_ok(), "{shell} should reserve under the consent: {reserved:?}");
    }
    assert_eq!(seen.len(), 3);
}

#[test]
fn a_cmd_first_approval_covers_powershell_too() {
    // The reverse direction, so the range cannot be an accident of which shell the
    // first case happened to use.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.cmd", "dir");
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    assert_eq!(
        grants[0].scope.consent.as_ref().expect("consent").approved.approved_shell,
        NativeShell::Cmd
    );
    let second = fixture.operation("shell.powershell", "Get-Date");
    let (_, decision) = fixture.derive(&second).expect("PowerShell reuses a CMD approval");
    let backend = fixture.native_backend();
    assert!(
        fixture
            .reserve(&second, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_ok()
    );
}

#[test]
fn another_principal_in_the_same_session_cannot_borrow_the_consent() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let other_snapshot = fixture.fresh_snapshot();
    let other = fixture.operation_in(
        "shell.powershell",
        "Get-Date",
        &other_snapshot,
        PrincipalId::new(),
        SecuritySourceRef::BuiltInTool {
            identity_hash: CanonicalSha256HashV1::sha256(b"builtin-shell"),
        },
    );
    let (_, decision) = fixture.derive(&other).expect("the grant is offered to the call");
    let backend = fixture.native_backend();
    assert!(
        fixture
            .reserve(&other, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_err(),
        "policy refuses a consent approved for a different principal"
    );
}

#[test]
fn an_mcp_source_is_never_covered_even_with_a_native_consent() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let mcp_snapshot = fixture.fresh_snapshot();
    let mcp = fixture.operation_in(
        "shell.powershell",
        "Get-Date",
        &mcp_snapshot,
        fixture.principal,
        SecuritySourceRef::McpServer { server_id: "server".into() },
    );
    let (_, decision) = fixture.derive(&mcp).expect("the grant is offered to the call");
    let backend = fixture.native_backend();
    assert!(
        fixture
            .reserve(&mcp, &backend, &decision, fixture.snapshot.session_permission_revision)
            .is_err(),
        "an MCP server cannot borrow a consent granted to the built-in Shell"
    );
}

#[test]
fn a_consent_reserves_a_new_operation_in_the_approved_revision() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let second = fixture.operation("shell.powershell", "Get-Process");
    let (_, decision) = fixture.derive(&second).expect("offered");
    let backend = fixture.native_backend();
    let reserved =
        fixture.reserve(&second, &backend, &decision, fixture.snapshot.session_permission_revision);
    assert!(reserved.is_ok(), "a fresh call under a live consent reserves: {reserved:?}");
}

#[test]
fn a_revoked_consent_cannot_reserve_and_a_replayed_approval_does_not_reissue_it() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let grant = grants[0].clone();
    fixture
        .store
        .database
        .unit_of_work(|tx| {
            security::revoke(
                tx,
                grant.grant_id,
                grant.revision,
                grant.scope.consent.as_ref(),
                fixture.at,
            )
        })
        .expect("revoke");
    let second = fixture.operation("shell.powershell", "Get-Process");
    assert!(fixture.derive(&second).is_none(), "a revoked consent is not offered to a new call");
}

#[test]
fn a_withdrawn_consent_is_returned_as_revoked_rather_than_reported_as_a_failure() {
    // The reported defect: the Store moved the Grant's revision, and the projection
    // still demanded the issued one, so a committed withdrawal was answered with
    // `permission.grant_not_applicable` and the dialog said the withdrawal had not
    // been submitted.
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let grants = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let grant = grants[0].clone();
    assert_eq!(
        fixture.consent_view(grant.grant_id).expect("live").availability,
        NativeSessionConsentAvailability::InForce
    );
    let reference = grant.scope.consent.clone().expect("consent");
    fixture
        .store
        .database
        .unit_of_work(|tx| {
            security::revoke(tx, grant.grant_id, grant.revision, Some(&reference), fixture.at)
        })
        .expect("revoke");
    let withdrawn = fixture
        .consent_view(grant.grant_id)
        .expect("a withdrawn consent is still a fact worth showing");
    assert_eq!(withdrawn.availability, NativeSessionConsentAvailability::Revoked);
    assert_eq!(
        withdrawn.grant_revision,
        Revision::new(2),
        "the revision a caller must present is the Grant's current one"
    );
    // The withdrawal is idempotent-rejecting: a retry presenting the stale revision
    // conflicts rather than reporting success twice.
    assert!(
        fixture
            .store
            .database
            .unit_of_work(|tx| security::revoke(
                tx,
                grant.grant_id,
                Revision::INITIAL,
                Some(&reference),
                fixture.at
            ))
            .is_err()
    );
}

#[test]
fn the_management_view_reports_liveness_and_refuses_a_mismatched_grant() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let first = fixture.operation("shell.powershell", "Get-Date");
    let issued = fixture.approve(&first, ApprovalDecisionKind::AllowSession);
    let grants = legacy::legacy(&mut fixture, issued);
    let grant = grants[0].clone();
    let consent =
        security::native_session_consent(fixture.store.database.connection(), grant.grant_id)
            .expect("consent")
            .expect("present");
    assert_eq!(
        fixture.consent_view(grant.grant_id).expect("live").scope.approved_shell,
        NativeShell::PowerShell
    );
    let mut foreign = grant.clone();
    foreign.grant_id = PermissionGrantId::new();
    assert!(
        project(
            &consent,
            &foreign,
            fixture.workspace,
            fixture.at,
            LiveFacts {
                runtime: fixture.runtime,
                permission_revision: fixture.snapshot.session_permission_revision
            },
            fixture.at
        )
        .is_none(),
        "a Grant that does not carry this consent is not shown as usable"
    );
    // A permission change invalidates the projection the same way it invalidates
    // dispatch.
    assert!(
        !fixture
            .consent_view_with(
                grant.grant_id,
                LiveFacts {
                    runtime: fixture.runtime,
                    permission_revision: fixture.snapshot.session_permission_revision + 1
                }
            )
            .expect("projection")
            .availability
            .is_in_force()
    );
}

/// Deriving a decision must not retarget an exact-operation Grant.
///
/// `AuthorizationScope::derive_for` replaces the operation fingerprint, and its contract
/// says a scope without a consent stays the stored scope verbatim. A caller that asks for
/// operation B while holding a Grant approved for A must not receive a decision that
/// claims B — even though policy and the reservation would refuse it later, an
/// authorization object that describes work the user never approved should not exist.
#[test]
fn an_exact_operation_grant_cannot_be_retargeted_without_consent() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let approved = fixture.operation("shell.cmd", "echo approved");
    let grant = fixture.approve(&approved, ApprovalDecisionKind::AllowOnce).remove(0);
    let other = fixture.operation("shell.cmd", "echo other");
    let expires = UtcTimestamp::from_value(fixture.at.value() + time::Duration::minutes(1));
    let derived = AuthorizationDecision::from_grant_for_operation(
        &grant,
        &other.fingerprint,
        fixture.at,
        expires,
    );
    assert!(
        derived.is_none(),
        "a Once Grant approved for one Operation cannot be derived for another"
    );
    // The same Operation still derives, and the decision is the stored scope verbatim.
    let same = AuthorizationDecision::from_grant_for_operation(
        &grant,
        &approved.fingerprint,
        fixture.at,
        expires,
    )
    .expect("the Operation the user approved still derives");
    assert_eq!(same.scope, grant.scope, "an exact-operation scope is never rewritten");
    assert!(same.scope.is_exact_operation());
}
