//! A Chrome site answered for the session. The Grant stands for the call that was answered; the
//! session's tabs count the site as confirmed for as long as the Grant lives, also after Core
//! restarts, and no other session does.
use super::*;
use aura_persistence_v2::chrome_sites;

fn chrome_call(
    fixture: &mut Fixture,
    capability: ResolvedCapability,
    origin: &str,
    confirmed: bool,
) -> SecuredOperation {
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
                identity_hash: CanonicalSha256HashV1::sha256(b"builtin-chrome"),
            },
            source_revision_fingerprint: None,
            capabilities: vec![capability],
            resources: vec![
                ResourceDescriptor::ChromeShare { share_id: format!("open:{}", fixture.session) },
                ResourceDescriptor::ChromeTab { tab_ref: "new".into() },
                ResourceDescriptor::ChromeOrigin { origin: origin.into(), confirmed },
            ],
            risks: vec![],
            credentials: vec![],
            purpose: None,
            action_kind: "chrome.open".into(),
            action_revision: Revision::INITIAL,
            action: serde_json::json!({ "op": "open", "url": format!("{origin}/") }),
        },
    )
    .expect("operation")
}

fn confirmed(fixture: &Fixture, session: SessionId) -> Vec<String> {
    chrome_sites::confirmed(fixture.store.database.connection(), session)
        .expect("sites")
        .into_iter()
        .collect()
}

#[test]
fn a_site_allowed_for_the_session_is_confirmed_for_that_session_until_its_grant_ends() {
    let mut fixture = Fixture::new(PermissionMode::WorkspaceReadWrite);
    let shop = "https://shop.example";
    let call = chrome_call(&mut fixture, ResolvedCapability::ChromeInteraction, shop, false);
    let grants = fixture.approve(&call, ApprovalDecisionKind::AllowSession);
    let [grant] = grants.as_slice() else { panic!("one grant: {grants:?}") };
    let consent = grant.scope.chrome_site_consent.as_ref().expect("site consent");
    assert_eq!(consent.origin, shop);
    assert_eq!(consent.session_id, fixture.session);
    assert_eq!(consent.approved_operation_fingerprint, call.fingerprint);
    assert!(grant.expires_at.is_none(), "it lasts as session answers do");
    assert!(grant.scope.is_exact_operation(), "the Grant itself covers the answered call only");
    assert_eq!(confirmed(&fixture, fixture.session), [shop]);
    assert!(confirmed(&fixture, SessionId::new()).is_empty(), "another session confirmed nothing");
    let at = fixture.at;
    fixture
        .store
        .database
        .unit_of_work(|tx| security::revoke(tx, grant.grant_id, grant.revision, None, at))
        .expect("revoked");
    assert!(confirmed(&fixture, fixture.session).is_empty(), "a revoked answer no longer counts");
}

#[test]
fn only_an_ordinary_call_on_an_unconfirmed_site_answered_for_the_session_confirms_it() {
    let mut fixture = Fixture::new(PermissionMode::WorkspaceReadWrite);
    let once = chrome_call(
        &mut fixture,
        ResolvedCapability::ChromeInteraction,
        "https://a.example",
        false,
    );
    let grants = fixture.approve(&once, ApprovalDecisionKind::AllowOnce);
    assert!(grants.iter().all(|grant| grant.scope.chrome_site_consent.is_none()));
    let sensitive = chrome_call(
        &mut fixture,
        ResolvedCapability::ChromeSensitiveAction,
        "https://b.example",
        false,
    );
    let grants = fixture.approve(&sensitive, ApprovalDecisionKind::AllowSession);
    assert!(grants.iter().all(|grant| grant.scope.chrome_site_consent.is_none()));
    let known =
        chrome_call(&mut fixture, ResolvedCapability::ChromeRead, "https://c.example", true);
    let grants = fixture.approve(&known, ApprovalDecisionKind::AllowSession);
    assert!(grants.iter().all(|grant| grant.scope.chrome_site_consent.is_none()));
    assert!(confirmed(&fixture, fixture.session).is_empty());
}
