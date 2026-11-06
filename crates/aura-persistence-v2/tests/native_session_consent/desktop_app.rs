//! A program answered for the session. The Grant stands for the call that was answered; the
//! session counts the program as authorized for as long as the Grant lives, also after Core
//! restarts, and no other session does. Stopping the program withdraws the session's answers for
//! it and no other program's.
use super::*;
use aura_persistence_v2::desktop_apps;

const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
const PAINT: &str = "Microsoft.Paint_8wekyb3d8bbwe!App";

fn computer_call(fixture: &mut Fixture, app_id: &str, authorized: bool) -> SecuredOperation {
    let snapshot = fixture.fresh_snapshot();
    let mut identity =
        InvocationIdentity::runtime_user(fixture.workspace, fixture.session, CorrelationId::new());
    identity.principal_id = fixture.principal;
    identity.agent_run_id = Some(fixture.run);
    identity.turn_task_id = Some(fixture.turn);
    identity.permission_snapshot_id = Some(snapshot.id);
    SecuredOperation::new(
        aura_domain_common::SecurityOperationId::new(),
        aura_execution_contract::OperationContent {
            compatibility: Default::default(),
            network: OperationNetwork::None,
            identity,
            source: SecuritySourceRef::BuiltInTool {
                identity_hash: CanonicalSha256HashV1::sha256(b"builtin-computer"),
            },
            source_revision_fingerprint: None,
            capabilities: vec![ResolvedCapability::DesktopInput],
            resources: vec![ResourceDescriptor::DesktopApp { app_id: app_id.into(), authorized }],
            risks: vec![],
            credentials: vec![],
            purpose: None,
            action_kind: "computer.act".into(),
            action_revision: Revision::INITIAL,
            action: serde_json::json!({ "app": app_id }),
        },
    )
    .expect("operation")
}

fn authorized(fixture: &Fixture, session: SessionId) -> Vec<String> {
    desktop_apps::authorized(fixture.store.database.connection(), session)
        .expect("programs")
        .into_iter()
        .collect()
}

#[test]
fn a_program_allowed_for_the_session_is_authorized_for_that_session_until_it_is_stopped() {
    let mut fixture = Fixture::new(PermissionMode::ReadOnly);
    let call = computer_call(&mut fixture, NOTEPAD, false);
    let grants = fixture.approve(&call, ApprovalDecisionKind::AllowSession);
    let [grant] = grants.as_slice() else { panic!("one grant: {grants:?}") };
    let consent = grant.scope.desktop_app_consent.as_ref().expect("program consent");
    assert_eq!(consent.app_id, NOTEPAD);
    assert_eq!(consent.session_id, fixture.session);
    assert_eq!(consent.approved_operation_fingerprint, call.fingerprint);
    assert!(grant.expires_at.is_none(), "it lasts as session answers do");
    assert!(grant.scope.is_exact_operation(), "the Grant itself covers the answered call only");
    let paint = computer_call(&mut fixture, PAINT, false);
    fixture.approve(&paint, ApprovalDecisionKind::AllowSession);
    assert_eq!(authorized(&fixture, fixture.session), [PAINT, NOTEPAD]);
    assert!(authorized(&fixture, SessionId::new()).is_empty(), "another session has none");

    let (session, at) = (fixture.session, fixture.at);
    let stopped = fixture
        .store
        .database
        .unit_of_work(|tx| desktop_apps::revoke(tx, session, NOTEPAD, at))
        .expect("stopped");
    assert_eq!(stopped, 1);
    assert_eq!(authorized(&fixture, fixture.session), [PAINT], "only the stopped one is gone");
    let again = fixture
        .store
        .database
        .unit_of_work(|tx| desktop_apps::revoke(tx, session, NOTEPAD, at))
        .expect("stopped again");
    assert_eq!(again, 0, "nothing left to withdraw");
}

#[test]
fn only_an_unauthorized_program_answered_for_the_session_is_authorized_by_it() {
    let mut fixture = Fixture::new(PermissionMode::FullAccess);
    let known = computer_call(&mut fixture, NOTEPAD, true);
    let grants = fixture.approve(&known, ApprovalDecisionKind::AllowSession);
    assert!(grants.iter().all(|grant| grant.scope.desktop_app_consent.is_none()));
    let denied = computer_call(&mut fixture, PAINT, false);
    let grants = fixture.approve(&denied, ApprovalDecisionKind::Deny);
    assert!(grants.iter().all(|grant| grant.scope.desktop_app_consent.is_none()));
    let once = computer_call(&mut fixture, PAINT, false);
    let grants = fixture.approve(&once, ApprovalDecisionKind::AllowOnce);
    assert!(!grants.is_empty(), "the answered call has its grant");
    assert!(grants.iter().all(|grant| grant.scope.desktop_app_consent.is_none()), "once only");
    assert!(authorized(&fixture, fixture.session).is_empty());
}
