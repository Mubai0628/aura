//! The user's own Chrome. Reading a shared tab is allowed in every mode, acting on it outside
//! ReadOnly; in ReadOnly the answer may cover the share for the rest of the turn. A site the user
//! has not confirmed is answered once or for the session; a sensitive action is answered once.
//! Chrome's own pages, credential, code, CAPTCHA, passkey and file-chooser actions are refused
//! whatever was approved, and so is a child agent. No other answer reaches Chrome, and a Chrome
//! answer reaches nothing else.
use super::*;
use ResolvedCapability as C;
use aura_security_contract::{
    ChromeSiteConsentRef, ChromeTurnConsentRef, NetworkCallerKind, NetworkConsentCaller,
    NetworkConsentDestinations, NetworkSessionConsentRef,
};

fn share(id: &str) -> ResourceDescriptor {
    ResourceDescriptor::ChromeShare { share_id: id.into() }
}
fn tab() -> ResourceDescriptor {
    ResourceDescriptor::ChromeTab { tab_ref: "t1".into() }
}
fn origin(value: &str, confirmed: bool) -> ResourceDescriptor {
    ResourceDescriptor::ChromeOrigin { origin: value.into(), confirmed }
}
const SITE: &str = "https://example.com";

fn chrome(
    mode: PermissionMode,
    action: &str,
    capabilities: Vec<C>,
    resources: Vec<ResourceDescriptor>,
) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.profile = SecurityProfile::for_permission_mode(mode);
    c.op.content.action_kind = action.into();
    c.op.content.action = serde_json::json!({});
    c.op.content.capabilities = capabilities;
    c.op.content.resources = resources;
    c.op.content.network = OperationNetwork::None;
    c.op.content.risks.clear();
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities = [C::ChromeRead, C::ChromeInteraction, C::ChromeSensitiveAction].into();
    c.backend.guarantees = [EnforcementGuarantee::AuthorizedWritesOnly].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}
fn on(page: Option<(&str, bool)>, share_id: &str) -> Vec<ResourceDescriptor> {
    let mut resources = vec![share(share_id), tab()];
    resources.extend(page.map(|(value, confirmed)| origin(value, confirmed)));
    resources
}
fn read(mode: PermissionMode, page: Option<(&str, bool)>) -> Case {
    chrome(mode, "chrome.snapshot", vec![C::ChromeRead], on(page, "share-1"))
}
fn act(mode: PermissionMode, page: Option<(&str, bool)>) -> Case {
    chrome(mode, "chrome.click", vec![C::ChromeInteraction], on(page, "share-1"))
}
fn sensitive(mode: PermissionMode, confirmed: bool) -> Case {
    chrome(
        mode,
        "chrome.click",
        vec![C::ChromeSensitiveAction],
        on(Some((SITE, confirmed)), "share-1"),
    )
}

fn decide(c: &Case) -> PolicyAuthorization {
    authorize(&c.input(), &c.backend, None)
}
fn allowed(c: &Case) -> bool {
    matches!(decide(c), PolicyAuthorization::Allow { .. })
}
fn asked(c: &Case) -> Vec<ApprovalDecisionKind> {
    match decide(c) {
        PolicyAuthorization::Ask { proposal } => proposal.allowed_decisions,
        other => panic!("expected a question, got {other:?}"),
    }
}
fn denied(c: &Case) -> PolicyDenyReason {
    match decide(c) {
        PolicyAuthorization::Deny { reason } => reason,
        other => panic!("expected a refusal, got {other:?}"),
    }
}
/// The next call of the same session and turn, with the grants it has.
fn next(c: &Case, mut call: Case) -> Case {
    call.snapshot = c.snapshot.clone();
    call.profile = c.profile;
    call.op.content.identity = c.op.content.identity.clone();
    call.grants = c.grants.clone();
    call.refresh();
    call
}
fn grant(c: &mut Case, scope: AuthorizationScope) -> aura_domain_common::PermissionGrantId {
    let grant_id = aura_domain_common::PermissionGrantId::new();
    c.grants.push(AuthorizationGrant {
        grant_id,
        revision: Revision::INITIAL,
        scope,
        issued_at: c.snapshot.created_at,
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    });
    grant_id
}
/// The production resolution of "allow for this turn" on a Chrome question.
fn turn_answer(c: &mut Case) {
    let PolicyAuthorization::Ask { proposal } = decide(c) else { panic!("ask") };
    let identity = c.op.content.identity.clone();
    let (session_id, turn_task_id) = (c.snapshot.session_id, identity.turn_task_id.expect("turn"));
    let mut scope = proposal.scope;
    scope.lifetime = AuthorizationLifetime::TurnTask { session_id, turn_task_id };
    scope.chrome_consent = Some(ChromeTurnConsentRef {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        session_id,
        turn_task_id,
        principal_id: identity.principal_id,
        share_id: "share-1".into(),
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        grant_revision: Revision::INITIAL,
    });
    grant(c, scope);
}

const ONCE: &[ApprovalDecisionKind] =
    &[ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny];
const SITE_ANSWERS: &[ApprovalDecisionKind] = &[
    ApprovalDecisionKind::AllowOnce,
    ApprovalDecisionKind::AllowSession,
    ApprovalDecisionKind::Deny,
];
const TURN: &[ApprovalDecisionKind] = &[
    ApprovalDecisionKind::AllowOnce,
    ApprovalDecisionKind::AllowForCurrentTurn,
    ApprovalDecisionKind::Deny,
];
const MODES: [PermissionMode; 3] =
    [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess];

#[test]
fn reading_a_shared_tab_never_asks_and_an_unconfirmed_site_is_answered_once_or_for_the_session() {
    for mode in MODES {
        assert!(allowed(&read(mode, None)), "{mode:?} without a site");
        assert!(allowed(&read(mode, Some((SITE, true)))), "{mode:?} confirmed");
        assert_eq!(asked(&read(mode, Some((SITE, false)))), SITE_ANSWERS, "{mode:?} unconfirmed");
    }
}

#[test]
fn acting_asks_only_in_read_only_where_the_answer_may_cover_the_turn() {
    assert_eq!(asked(&act(PermissionMode::ReadOnly, None)), TURN);
    assert_eq!(asked(&act(PermissionMode::ReadOnly, Some((SITE, true)))), TURN);
    for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
        assert!(allowed(&act(mode, None)), "{mode:?}");
        assert!(allowed(&act(mode, Some((SITE, true)))), "{mode:?} confirmed");
    }
    for mode in MODES {
        assert_eq!(asked(&act(mode, Some((SITE, false)))), SITE_ANSWERS, "{mode:?} unconfirmed");
    }
}

#[test]
fn a_sensitive_action_asks_every_time_and_only_once_may_be_allowed() {
    for mode in MODES {
        assert_eq!(asked(&sensitive(mode, true)), ONCE, "{mode:?}");
        assert_eq!(asked(&sensitive(mode, false)), ONCE, "{mode:?} unconfirmed");
    }
}

#[test]
fn a_turn_answer_covers_its_share_for_the_rest_of_the_turn_only() {
    let mut first = act(PermissionMode::ReadOnly, Some((SITE, true)));
    turn_answer(&mut first);
    assert!(allowed(&first), "the approved call");
    assert!(allowed(&next(&first, act(PermissionMode::ReadOnly, Some((SITE, true))))), "again");
    assert!(allowed(&next(&first, read(PermissionMode::ReadOnly, None))), "reading it");
    let typing = chrome(
        PermissionMode::ReadOnly,
        "chrome.type",
        vec![C::ChromeInteraction],
        on(None, "share-1"),
    );
    assert!(allowed(&next(&first, typing)), "another action on the same share");

    let other_share = chrome(
        PermissionMode::ReadOnly,
        "chrome.click",
        vec![C::ChromeInteraction],
        on(None, "share-2"),
    );
    assert_eq!(asked(&next(&first, other_share)), TURN, "another share");
    assert_eq!(
        asked(&next(&first, act(PermissionMode::ReadOnly, Some((SITE, false))))),
        SITE_ANSWERS,
        "a new site"
    );
    assert_eq!(asked(&next(&first, sensitive(PermissionMode::ReadOnly, true))), ONCE, "sensitive");

    let mut later = next(&first, act(PermissionMode::ReadOnly, Some((SITE, true))));
    later.op.content.identity.turn_task_id = Some(aura_domain_common::TurnTaskId::new());
    later.refresh();
    assert_eq!(asked(&later), TURN, "the next turn");
}

/// The production resolution of "allow this site for the session" on a Chrome question.
fn site_answer(c: &mut Case) {
    let PolicyAuthorization::Ask { proposal } = decide(c) else { panic!("ask") };
    let session_id = c.snapshot.session_id;
    let mut scope = proposal.scope;
    scope.lifetime = AuthorizationLifetime::Session { session_id };
    scope.chrome_site_consent = Some(ChromeSiteConsentRef {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        session_id,
        principal_id: c.op.content.identity.principal_id,
        origin: SITE.into(),
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        grant_revision: Revision::INITIAL,
    });
    grant(c, scope);
}

#[test]
fn a_session_site_answer_allows_the_answered_call_and_the_mode_decides_the_rest() {
    let mut first = act(PermissionMode::WorkspaceReadWrite, Some((SITE, false)));
    site_answer(&mut first);
    assert!(allowed(&first), "the answered call");
    // Core resolves the site as confirmed for the session's later calls; the Grant itself
    // covers no other call.
    let typing = chrome(
        PermissionMode::WorkspaceReadWrite,
        "chrome.type",
        vec![C::ChromeInteraction],
        on(Some((SITE, false)), "share-1"),
    );
    assert_eq!(asked(&next(&first, typing)), SITE_ANSWERS, "the Grant is not reused");
    let confirmed = act(PermissionMode::WorkspaceReadWrite, Some((SITE, true)));
    assert!(allowed(&next(&first, confirmed)), "a confirmed site in a working mode");
    assert_eq!(
        asked(&next(&first, sensitive(PermissionMode::WorkspaceReadWrite, true))),
        ONCE,
        "a sensitive action still asks every time"
    );
    let mut read_only = act(PermissionMode::ReadOnly, Some((SITE, false)));
    site_answer(&mut read_only);
    let acting = act(PermissionMode::ReadOnly, Some((SITE, true)));
    assert_eq!(asked(&next(&read_only, acting)), TURN, "ReadOnly still asks before acting");
}

/// A click in the built-in browser, in ReadOnly: a question of the browser's own.
fn browser_click() -> Case {
    let mut browser = Case::new(false);
    browser.snapshot.mode = PermissionMode::ReadOnly;
    browser.profile = SecurityProfile::for_permission_mode(PermissionMode::ReadOnly);
    browser.caller = NetworkCallerKind::AuraBrowser;
    browser.op.content.action_kind = "browser.click".into();
    browser.op.content.action = serde_json::json!({});
    browser.op.content.capabilities = vec![C::BrowserInteraction];
    browser.op.content.resources = vec![
        ResourceDescriptor::BrowserPage { page_ref: "model".into() },
        ResourceDescriptor::BrowserSite { host: "example.com".into(), allowed_by_rule: None },
    ];
    browser.op.content.network = OperationNetwork::BrowserSitePolicy;
    browser.backend.class = BackendClass::CoreBroker;
    browser.backend.capabilities = [C::BrowserRead, C::BrowserInteraction].into();
    browser.backend.guarantees = [EnforcementGuarantee::AuthorizedWritesOnly].into();
    browser.backend.known_limitations.clear();
    browser.refresh();
    browser
}

#[test]
fn no_other_answer_reaches_chrome_and_a_chrome_answer_reaches_nothing_else() {
    let browser = browser_click();
    for (caller, destinations) in [
        (
            NetworkConsentCaller::AuraBrowser,
            NetworkConsentDestinations::BrowserSite { host: "example.com".into() },
        ),
        (NetworkConsentCaller::OrdinaryTools, NetworkConsentDestinations::AllOrdinaryDestinations),
    ] {
        let mut answered = browser_click();
        answered.op.content.identity = browser.op.content.identity.clone();
        answered.snapshot = browser.snapshot.clone();
        answered.refresh();
        let PolicyAuthorization::Ask { proposal } = decide(&answered) else {
            panic!("browser asks")
        };
        let session_id = answered.snapshot.session_id;
        let mut scope = proposal.scope;
        scope.lifetime = AuthorizationLifetime::Session { session_id };
        let grant_id = aura_domain_common::PermissionGrantId::new();
        scope.network_consent = Some(NetworkSessionConsentRef {
            version: aura_domain_common::ToolWireVersionV1,
            approval_id: aura_domain_common::ApprovalRequestId::new(),
            grant_id,
            grant_revision: Revision::INITIAL,
            workspace_id: answered.snapshot.workspace_id,
            session_id,
            principal_id: answered.op.content.identity.principal_id,
            caller,
            destinations,
            approved_operation_fingerprint: answered.op.fingerprint.clone(),
            revoked_grant_revision: None,
        });
        answered.grants.push(AuthorizationGrant {
            grant_id,
            revision: Revision::INITIAL,
            scope,
            issued_at: answered.snapshot.created_at,
            expires_at: None,
            revoked_at: None,
            consumed_at: None,
        });
        let chrome_call = next(&answered, act(PermissionMode::ReadOnly, Some((SITE, true))));
        assert_eq!(asked(&chrome_call), TURN, "{caller:?} answer");
    }

    let mut chrome_turn = act(PermissionMode::ReadOnly, None);
    turn_answer(&mut chrome_turn);
    let after = next(&chrome_turn, browser_click());
    assert!(
        matches!(decide(&after), PolicyAuthorization::Ask { .. }),
        "a Chrome answer and the browser"
    );
}

#[test]
fn a_missing_share_or_a_wrong_shape_is_not_an_operation() {
    let bad = |c: Case| {
        assert_eq!(denied(&c), PolicyDenyReason::InvalidOperation, "{}", c.op.content.action_kind)
    };
    for mode in MODES {
        bad(chrome(mode, "chrome.snapshot", vec![C::ChromeRead], vec![tab()]));
        bad(chrome(mode, "chrome.snapshot", vec![C::ChromeRead], vec![share(""), tab()]));
        bad(chrome(
            mode,
            "chrome.snapshot",
            vec![C::ChromeRead],
            vec![share("one"), share("two"), tab()],
        ));
        bad(chrome(mode, "chrome.open", vec![C::ChromeInteraction], on(None, "share-1")));
        bad(chrome(
            mode,
            "chrome.click",
            vec![C::ChromeInteraction, C::BrowserInteraction],
            on(None, "share-1"),
        ));
        // Listing the shares reads Core's own record; it never carries Chrome's authority.
        bad(chrome(mode, "chrome.tabs", vec![C::ChromeRead], on(None, "share-1")));
        let mut networked = act(mode, None);
        networked.op.content.network = OperationNetwork::BrowserSitePolicy;
        networked.refresh();
        bad(networked);
        let mut ordinary = act(mode, None);
        ordinary.op.content.resources.push(ResourceDescriptor::NetworkOrigin {
            scheme: "https".into(),
            host: "example.com".into(),
            port: 443,
        });
        ordinary.refresh();
        bad(ordinary);
        // Another tool may not borrow a Chrome capability.
        let mut borrowed = Case::new(false);
        borrowed.snapshot.mode = mode;
        borrowed.op.content.capabilities = vec![C::ChromeRead];
        borrowed.op.content.resources = on(Some((SITE, true)), "share-1");
        borrowed.refresh();
        assert_eq!(denied(&borrowed), PolicyDenyReason::InvalidOperation, "another tool");
    }
}

#[test]
fn chrome_pages_and_protected_fields_are_refused_even_with_an_approval() {
    let refused = |mut c: Case| {
        let mut scope = asked_scope(&c);
        scope.lifetime = AuthorizationLifetime::Session { session_id: c.snapshot.session_id };
        grant(&mut c, scope);
        assert_eq!(denied(&c), PolicyDenyReason::HardBoundary, "{:?}", c.op.content);
    };
    for mode in MODES {
        for page in [
            "chrome://settings",
            "chrome-extension://abcdefghijklmnop/page.html",
            "devtools://devtools/bundled/inspector.html",
            "https://chromewebstore.google.com/detail/example",
            "https://chrome.google.com/webstore/detail/example",
        ] {
            refused(read(mode, Some((page, true))));
        }
        for risk in [
            OperationRiskFlag::ChromeCredentialField,
            OperationRiskFlag::ChromeCaptcha,
            OperationRiskFlag::ChromePasskey,
            OperationRiskFlag::ChromeFileChooser,
        ] {
            let mut c = act(mode, Some((SITE, true)));
            c.op.content.risks = vec![risk];
            c.refresh();
            refused(c);
        }
    }
}

/// An approval scope shaped like the answer to an ordinary Chrome question.
fn asked_scope(c: &Case) -> AuthorizationScope {
    let mut question = read(c.snapshot.mode, Some((SITE, false)));
    question.snapshot = c.snapshot.clone();
    question.op.content.identity = c.op.content.identity.clone();
    question.refresh();
    let PolicyAuthorization::Ask { proposal } = decide(&question) else { panic!("ask") };
    let mut scope = proposal.scope;
    scope.operation_fingerprint = c.op.fingerprint.clone();
    scope
}

#[test]
fn a_child_agent_cannot_use_a_shared_tab() {
    for mode in MODES {
        let mut c = read(mode, Some((SITE, true)));
        c.op.content.identity.actor = InvocationActor::ChildAgent;
        c.op.content.identity.parent_agent_run_id = Some(aura_domain_common::AgentRunId::new());
        c.refresh();
        assert_eq!(denied(&c), PolicyDenyReason::ModeDenied, "{mode:?}");
    }
}

#[test]
fn handing_a_tab_to_the_user_never_asks() {
    for mode in MODES {
        let handing = chrome(
            mode,
            "chrome.request_user",
            vec![C::ChromeRead],
            on(Some((SITE, true)), "share-1"),
        );
        assert!(allowed(&handing), "{mode:?}: {:?}", decide(&handing));
    }
}

/// Listing the shares reads Core's own record of them as task state: allowed in every mode, like
/// any task read, and nothing about it is Chrome's.
#[test]
fn listing_the_shares_is_a_task_read() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let mut listing = chrome(mode, "chrome.tabs", vec![C::AgentTaskRead], vec![tab()]);
        let identity = listing.op.content.identity.clone();
        listing.op.content.resources = vec![ResourceDescriptor::AgentTask {
            session_id: identity.session_id.expect("session"),
            turn_task_id: identity.turn_task_id.expect("turn"),
            agent_run_id: identity.agent_run_id.expect("run"),
        }];
        listing.backend.capabilities = [C::AgentTaskRead, C::AgentTaskWrite].into();
        listing.refresh();
        assert!(allowed(&listing), "{mode:?}: {:?}", decide(&listing));
    }
}
