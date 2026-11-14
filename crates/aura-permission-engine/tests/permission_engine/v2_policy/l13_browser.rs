//! The built-in browser: reading the page never asks;
//! opening a site is the mode's default except in ReadOnly, where only the user's allowed sites
//! open without asking; acting on the page asks in ReadOnly; a sensitive action asks every time.
//! A session answer names one site and one kind of access, and ordinary network answers never
//! reach the browser. Nothing else may borrow the browser's shape.
use super::*;
use ResolvedCapability as C;
use aura_security_contract::{
    NetworkCallerKind, NetworkConsentCaller, NetworkConsentDestinations, NetworkSessionConsentRef,
};

fn page() -> ResourceDescriptor {
    ResourceDescriptor::BrowserPage { page_ref: "model".into() }
}
fn site(host: &str, rule: Option<&str>) -> ResourceDescriptor {
    ResourceDescriptor::BrowserSite { host: host.into(), allowed_by_rule: rule.map(Into::into) }
}
fn origin(host: &str) -> ResourceDescriptor {
    ResourceDescriptor::NetworkOrigin { scheme: "https".into(), host: host.into(), port: 443 }
}

fn browser(
    mode: PermissionMode,
    action: &str,
    capabilities: Vec<ResolvedCapability>,
    resources: Vec<ResourceDescriptor>,
    network: OperationNetwork,
) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.profile = SecurityProfile::for_permission_mode(mode);
    c.caller = NetworkCallerKind::AuraBrowser;
    c.op.content.action_kind = action.into();
    c.op.content.action = serde_json::json!({});
    c.op.content.capabilities = capabilities;
    c.op.content.resources = resources;
    c.op.content.network = network;
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities =
        [C::BrowserRead, C::BrowserInteraction, C::BrowserSensitiveAction, C::WorkspaceFileRead]
            .into();
    c.backend.guarantees = [EnforcementGuarantee::AuthorizedWritesOnly].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}
fn open(mode: PermissionMode, host: &str, rule: Option<&str>) -> Case {
    browser(
        mode,
        "browser.open",
        vec![C::BrowserRead],
        vec![page(), site(host, rule), origin(host)],
        OperationNetwork::BrowserSitePolicy,
    )
}
fn read(mode: PermissionMode, host: &str) -> Case {
    browser(
        mode,
        "browser.snapshot",
        vec![C::BrowserRead],
        vec![page(), site(host, None)],
        OperationNetwork::None,
    )
}
fn act(mode: PermissionMode, host: &str) -> Case {
    browser(
        mode,
        "browser.click",
        vec![C::BrowserInteraction],
        vec![page(), site(host, None)],
        OperationNetwork::BrowserSitePolicy,
    )
}

fn decide(c: &Case) -> PolicyAuthorization {
    authorize(&c.input(), &c.backend, None)
}
fn allowed(c: &Case) -> bool {
    matches!(decide(c), PolicyAuthorization::Allow { .. })
}
fn proposal(c: &Case) -> AuthorizationProposal {
    match decide(c) {
        PolicyAuthorization::Ask { proposal } => proposal,
        other => panic!("expected a question, got {other:?}"),
    }
}

/// The production resolution of "allow this site for the session".
fn session_answer(c: &mut Case, host: &str, caller: NetworkConsentCaller) {
    let mut scope = proposal(c).scope;
    let session_id = c.snapshot.session_id;
    let grant_id = aura_domain_common::PermissionGrantId::new();
    scope.lifetime = AuthorizationLifetime::Session { session_id };
    scope.network_consent = Some(NetworkSessionConsentRef {
        version: aura_domain_common::ToolWireVersionV1,
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        grant_id,
        grant_revision: Revision::INITIAL,
        workspace_id: c.snapshot.workspace_id,
        session_id,
        principal_id: c.op.content.identity.principal_id,
        caller,
        destinations: if caller == NetworkConsentCaller::AuraBrowser {
            NetworkConsentDestinations::BrowserSite { host: host.into() }
        } else {
            NetworkConsentDestinations::AllOrdinaryDestinations
        },
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        revoked_grant_revision: None,
    });
    c.grants.push(AuthorizationGrant {
        grant_id,
        revision: Revision::INITIAL,
        scope,
        issued_at: c.snapshot.created_at,
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    });
}

/// The next call of the same session, with the grants it has.
fn next(c: &Case, mut call: Case) -> Case {
    call.snapshot = c.snapshot.clone();
    call.op.content.identity = c.op.content.identity.clone();
    call.grants = c.grants.clone();
    call.refresh();
    call
}

#[test]
fn opening_a_site_follows_the_mode_and_read_only_follows_the_allowed_sites() {
    for mode in [PermissionMode::FullAccess, PermissionMode::WorkspaceReadWrite] {
        assert!(
            allowed(&open(mode, "example.com", None)),
            "{mode:?}: {:?}",
            decide(&open(mode, "example.com", None))
        );
    }
    let asked = proposal(&open(PermissionMode::ReadOnly, "example.com", None));
    assert_eq!(asked.requested_capabilities, Some(vec![C::BrowserRead]));
    assert_eq!(
        asked.allowed_decisions,
        vec![
            ApprovalDecisionKind::AllowOnce,
            ApprovalDecisionKind::AllowSession,
            ApprovalDecisionKind::Deny
        ]
    );
    assert!(allowed(&open(PermissionMode::ReadOnly, "docs.rs", Some("docs.rs"))));
}

#[test]
fn reading_the_page_never_asks_and_acting_on_it_asks_only_in_read_only() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        assert!(allowed(&read(mode, "example.com")), "{mode:?}");
    }
    assert!(allowed(&act(PermissionMode::WorkspaceReadWrite, "example.com")));
    assert!(allowed(&act(PermissionMode::FullAccess, "example.com")));
    assert_eq!(
        proposal(&act(PermissionMode::ReadOnly, "example.com")).requested_capabilities,
        Some(vec![C::BrowserInteraction])
    );
    // An allowed site opens without asking; acting on it is still a question in ReadOnly.
    let mut acting = act(PermissionMode::ReadOnly, "docs.rs");
    acting.op.content.resources = vec![page(), site("docs.rs", Some("docs.rs"))];
    acting.refresh();
    assert!(matches!(decide(&acting), PolicyAuthorization::Ask { .. }));
}

#[test]
fn a_sensitive_action_asks_every_time_in_every_mode() {
    for mode in
        [PermissionMode::FullAccess, PermissionMode::WorkspaceReadWrite, PermissionMode::ReadOnly]
    {
        let mut c = browser(
            mode,
            "browser.evaluate",
            vec![C::BrowserSensitiveAction],
            vec![page(), site("bank.example", None)],
            OperationNetwork::BrowserSitePolicy,
        );
        c.op.content.risks = vec![OperationRiskFlag::SensitiveBrowserAction];
        c.refresh();
        assert_eq!(
            proposal(&c).allowed_decisions,
            vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
            "{mode:?}"
        );
    }
}

#[test]
fn a_session_answer_covers_its_site_and_its_kind_of_access_only() {
    let mut first = open(PermissionMode::ReadOnly, "example.com", None);
    session_answer(&mut first, "example.com", NetworkConsentCaller::AuraBrowser);
    assert!(allowed(&first), "the approved call");
    let again = next(&first, open(PermissionMode::ReadOnly, "example.com", None));
    assert!(allowed(&again), "another call opening the same site");
    let elsewhere = next(&first, open(PermissionMode::ReadOnly, "other.example", None));
    assert!(matches!(decide(&elsewhere), PolicyAuthorization::Ask { .. }), "another site");
    let acting = next(&first, act(PermissionMode::ReadOnly, "example.com"));
    assert!(
        matches!(decide(&acting), PolicyAuthorization::Ask { .. }),
        "acting is its own question"
    );
    let mut acting = acting;
    session_answer(&mut acting, "example.com", NetworkConsentCaller::AuraBrowser);
    let clicking_again = next(&acting, act(PermissionMode::ReadOnly, "example.com"));
    assert!(allowed(&clicking_again));
}

#[test]
fn an_ordinary_network_answer_never_reaches_the_browser() {
    let mut web = Case::new(false);
    web.snapshot.mode = PermissionMode::ReadOnly;
    let mut c = open(PermissionMode::ReadOnly, "example.com", None);
    // Hand-built: an "allow network for this session" answer of the ordinary tools.
    session_answer(&mut c, "example.com", NetworkConsentCaller::OrdinaryTools);
    let after = next(&c, open(PermissionMode::ReadOnly, "example.com", None));
    assert!(matches!(decide(&after), PolicyAuthorization::Ask { .. }));
    drop(web);
}

#[test]
fn a_page_without_a_site_offers_no_session_answer() {
    let c = browser(
        PermissionMode::ReadOnly,
        "browser.click",
        vec![C::BrowserInteraction],
        vec![page()],
        OperationNetwork::BrowserSitePolicy,
    );
    assert_eq!(
        proposal(&c).allowed_decisions,
        vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );
    // Opening a workspace file is a workspace read: no mode asks.
    let file = browser(
        PermissionMode::ReadOnly,
        "browser.open",
        vec![C::BrowserRead, C::WorkspaceFileRead],
        vec![
            page(),
            ResourceDescriptor::WorkspacePath {
                workspace_id: c.snapshot.workspace_id,
                canonical_relative_path: "index.html".parse().expect("path"),
                file_target: None,
            },
        ],
        OperationNetwork::BrowserSitePolicy,
    );
    assert!(allowed(&file), "{:?}", decide(&file));
}

#[test]
fn nothing_else_may_borrow_the_browser_shape() {
    let refused = |c: &Case| {
        matches!(
            decide(c),
            PolicyAuthorization::Deny { reason: PolicyDenyReason::InvalidOperation }
        )
    };
    let mut wrong_caller = open(PermissionMode::FullAccess, "example.com", None);
    wrong_caller.caller = NetworkCallerKind::AuraTool;
    assert!(refused(&wrong_caller), "a browser call from another caller");
    let mut ordinary = open(PermissionMode::FullAccess, "example.com", None);
    ordinary.op.content.capabilities.push(C::OrdinaryNetwork);
    ordinary.refresh();
    assert!(refused(&ordinary), "ordinary network in a browser call");
    let mut mismatched = open(PermissionMode::FullAccess, "example.com", None);
    mismatched.op.content.resources =
        vec![page(), site("example.com", None), origin("other.example")];
    mismatched.refresh();
    assert!(refused(&mismatched), "a target the site does not name");
    let mut exact = open(PermissionMode::FullAccess, "example.com", None);
    exact.op.content.network = OperationNetwork::ExactOrigins;
    exact.refresh();
    assert!(refused(&exact), "exact-origin network for a page");
    let mut borrowed = Case::new(false);
    borrowed.op.content.network = OperationNetwork::BrowserSitePolicy;
    borrowed.refresh();
    assert!(refused(&borrowed), "the browser's network declaration outside the browser");
}
