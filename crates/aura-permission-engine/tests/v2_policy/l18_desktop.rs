//! Programs on the user's desktop. A program the session has authorized is read and acted on
//! without asking in every mode; any other program asks in every mode, answered for the session
//! or not at all. A session answer stands for the answered call; later calls count the program as
//! authorized when Core resolves them. A wrong shape and a child agent are refused, no other answer
//! reaches a program, and a program's answer reaches nothing else.
use super::*;
use ResolvedCapability as C;
use aura_security_contract::{
    ChromeSiteConsentRef, DesktopAppConsentRef, NetworkConsentCaller, NetworkConsentDestinations,
    NetworkSessionConsentRef,
};

const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
const PAINT: &str = "Microsoft.Paint_8wekyb3d8bbwe!App";

fn app(id: &str, authorized: bool) -> ResourceDescriptor {
    ResourceDescriptor::DesktopApp { app_id: id.into(), authorized }
}

fn computer(
    mode: PermissionMode,
    action: &str,
    capabilities: Vec<C>,
    resources: Vec<ResourceDescriptor>,
) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.profile = SecurityProfile::for_permission_mode(mode);
    c.op.content.action_kind = action.into();
    c.op.content.action = serde_json::json!({"safety": "clear"});
    c.op.content.capabilities = capabilities;
    c.op.content.resources = resources;
    c.op.content.network = OperationNetwork::None;
    c.op.content.risks.clear();
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities = [C::DesktopRead, C::DesktopInput].into();
    c.backend.guarantees = [EnforcementGuarantee::AuthorizedWritesOnly].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}
fn read(mode: PermissionMode, id: &str, authorized: bool) -> Case {
    computer(mode, "computer.snapshot", vec![C::DesktopRead], vec![app(id, authorized)])
}
fn act(mode: PermissionMode, id: &str, authorized: bool) -> Case {
    computer(mode, "computer.act", vec![C::DesktopInput], vec![app(id, authorized)])
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
fn grant(c: &mut Case, scope: AuthorizationScope) {
    c.grants.push(AuthorizationGrant {
        grant_id: aura_domain_common::PermissionGrantId::new(),
        revision: Revision::INITIAL,
        scope,
        issued_at: c.snapshot.created_at,
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    });
}
/// The production resolution of "allow this program for the session".
fn app_answer(c: &mut Case, id: &str) {
    let PolicyAuthorization::Ask { proposal } = decide(c) else { panic!("ask") };
    let session_id = c.snapshot.session_id;
    let mut scope = proposal.scope;
    scope.lifetime = AuthorizationLifetime::Session { session_id };
    scope.desktop_app_consent = Some(DesktopAppConsentRef {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        session_id,
        principal_id: c.op.content.identity.principal_id,
        app_id: id.into(),
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        grant_revision: Revision::INITIAL,
    });
    grant(c, scope);
}

const ANSWERS: &[ApprovalDecisionKind] =
    &[ApprovalDecisionKind::AllowSession, ApprovalDecisionKind::Deny];

#[test]
fn password_writes_are_denied_even_with_exact_or_session_approval_in_every_mode() {
    for mode in MODES {
        for authorized in [false, true] {
            let mut c = act(mode, NOTEPAD, false);
            let PolicyAuthorization::Ask { proposal } = decide(&c) else { panic!("ask") };
            c.op.content.resources = vec![app(NOTEPAD, authorized)];
            c.op.content.action =
                serde_json::json!({"command": {"op": "act"}, "safety": "password"});
            c.refresh();
            assert_eq!(denied(&c), PolicyDenyReason::ModeDenied);
            let mut scope = proposal.scope;
            scope.operation_fingerprint = c.op.fingerprint.clone();
            grant(&mut c, scope);
            assert_eq!(denied(&c), PolicyDenyReason::ModeDenied);
        }
    }
}

#[test]
fn payment_or_uninspected_input_cannot_be_released_by_any_mode_or_approval() {
    for mode in MODES {
        for verdict in [
            serde_json::json!("payment"),
            serde_json::json!("unavailable"),
            serde_json::Value::Null,
        ] {
            let mut c = act(mode, NOTEPAD, false);
            let PolicyAuthorization::Ask { proposal } = decide(&c) else { panic!("ask") };
            c.op.content.resources = vec![app(NOTEPAD, true)];
            c.op.content.action = serde_json::json!({"safety": verdict});
            c.refresh();
            let mut scope = proposal.scope;
            scope.operation_fingerprint = c.op.fingerprint.clone();
            grant(&mut c, scope);
            assert_eq!(denied(&c), PolicyDenyReason::ModeDenied);
        }
    }
}
const MODES: [PermissionMode; 3] =
    [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess];

#[test]
fn a_program_the_session_has_not_authorized_asks_in_every_mode_for_the_session_or_not() {
    for mode in MODES {
        assert_eq!(asked(&read(mode, NOTEPAD, false)), ANSWERS, "{mode:?} reading");
        assert_eq!(asked(&act(mode, NOTEPAD, false)), ANSWERS, "{mode:?} acting");
        let opening =
            computer(mode, "computer.open", vec![C::DesktopInput], vec![app(NOTEPAD, false)]);
        assert_eq!(asked(&opening), ANSWERS, "{mode:?} opening");
    }
}

#[test]
fn an_authorized_program_is_read_and_acted_on_without_asking_in_every_mode() {
    for mode in MODES {
        assert!(allowed(&read(mode, NOTEPAD, true)), "{mode:?} reading");
        assert!(allowed(&act(mode, NOTEPAD, true)), "{mode:?} acting");
        let both = computer(
            mode,
            "computer.input",
            vec![C::DesktopRead, C::DesktopInput],
            vec![app(NOTEPAD, true)],
        );
        assert!(allowed(&both), "{mode:?} acting with a screenshot after");
    }
}

#[test]
fn a_session_answer_allows_the_answered_call_and_core_decides_the_rest() {
    for mode in MODES {
        let mut first = act(mode, NOTEPAD, false);
        app_answer(&mut first, NOTEPAD);
        assert!(allowed(&first), "{mode:?} the answered call");
        // The Grant covers no other call; Core resolves the program as authorized for the
        // session's later calls.
        let again = next(&first, read(mode, NOTEPAD, false));
        assert_eq!(asked(&again), ANSWERS, "{mode:?} the Grant is not reused");
        assert!(allowed(&next(&first, read(mode, NOTEPAD, true))), "{mode:?} resolved");
        assert_eq!(asked(&next(&first, act(mode, PAINT, false))), ANSWERS, "{mode:?} another");
    }
}

#[test]
fn a_program_answer_does_not_reach_another_session_or_outlive_its_withdrawal() {
    let mut first = act(PermissionMode::FullAccess, NOTEPAD, false);
    app_answer(&mut first, NOTEPAD);
    let mut elsewhere = next(&first, act(PermissionMode::FullAccess, NOTEPAD, false));
    let other = aura_domain_common::SessionId::new();
    elsewhere.snapshot.session_id = other;
    elsewhere.op.content.identity.session_id = Some(other);
    elsewhere.refresh();
    assert_eq!(asked(&elsewhere), ANSWERS, "another session");

    first.grants[0].revoked_at = Some(first.snapshot.created_at);
    first.refresh();
    assert_eq!(asked(&first), ANSWERS, "a withdrawn answer");
}

/// The contract's own reading of a program's answer, whatever reaches it: the policy never
/// derives such an answer for another call, but the answer must not stand for one either.
#[test]
fn a_program_answer_stands_for_its_own_call_of_its_own_session_while_it_lives() {
    let mut first = act(PermissionMode::FullAccess, NOTEPAD, false);
    app_answer(&mut first, NOTEPAD);
    let grant = first.grants[0].clone();
    let covers = |c: &Case, grant: &AuthorizationGrant| {
        let content = &c.op.content;
        aura_security_contract::desktop_app_consent_covers(
            grant,
            &content.identity,
            &content.source,
            &content.action_kind,
            &content.capabilities,
            &content.resources,
        )
    };
    assert!(covers(&first, &grant), "the answered call");
    let other = next(&first, read(PermissionMode::FullAccess, NOTEPAD, false));
    let mut derived = grant.clone();
    derived.scope.operation_fingerprint = other.op.fingerprint.clone();
    assert!(!covers(&other, &derived), "the answer moved to another call");
    let mut elsewhere = next(&first, act(PermissionMode::FullAccess, NOTEPAD, false));
    let session = aura_domain_common::SessionId::new();
    elsewhere.snapshot.session_id = session;
    elsewhere.op.content.identity.session_id = Some(session);
    elsewhere.refresh();
    let mut moved = grant.clone();
    moved.scope.operation_fingerprint = elsewhere.op.fingerprint.clone();
    if let Some(consent) = moved.scope.desktop_app_consent.as_mut() {
        consent.approved_operation_fingerprint = elsewhere.op.fingerprint.clone();
    }
    assert!(!covers(&elsewhere, &moved), "another session");
    let mut withdrawn = grant.clone();
    withdrawn.revoked_at = Some(first.snapshot.created_at);
    assert!(!covers(&first, &withdrawn), "a withdrawn answer");
    let mut paint = next(&first, act(PermissionMode::FullAccess, PAINT, false));
    paint.refresh();
    let mut other_program = grant;
    other_program.scope.operation_fingerprint = paint.op.fingerprint.clone();
    if let Some(consent) = other_program.scope.desktop_app_consent.as_mut() {
        consent.approved_operation_fingerprint = paint.op.fingerprint.clone();
    }
    assert!(!covers(&paint, &other_program), "another program");
}

/// A Chrome click on a site the share has not confirmed: a Chrome question in every mode.
fn chrome_click() -> Case {
    let mut c = computer(
        PermissionMode::ReadOnly,
        "chrome.click",
        vec![C::ChromeInteraction],
        vec![
            ResourceDescriptor::ChromeShare { share_id: "share-1".into() },
            ResourceDescriptor::ChromeTab { tab_ref: "t1".into() },
            ResourceDescriptor::ChromeOrigin {
                origin: "https://example.com".into(),
                confirmed: false,
            },
        ],
    );
    c.backend.capabilities = [C::ChromeRead, C::ChromeInteraction].into();
    c.refresh();
    c
}

#[test]
fn no_other_answer_reaches_a_program_and_a_program_answer_reaches_nothing_else() {
    let call = act(PermissionMode::FullAccess, NOTEPAD, false);
    let session_id = call.snapshot.session_id;
    let principal_id = call.op.content.identity.principal_id;
    let PolicyAuthorization::Ask { proposal } = decide(&call) else { panic!("ask") };
    // Answers of other kinds, each written for exactly this call.
    let mut others = Vec::new();
    let mut scope = proposal.scope.clone();
    scope.lifetime = AuthorizationLifetime::Session { session_id };
    scope.chrome_site_consent = Some(ChromeSiteConsentRef {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        session_id,
        principal_id,
        origin: "https://example.com".into(),
        approved_operation_fingerprint: call.op.fingerprint.clone(),
        grant_revision: Revision::INITIAL,
    });
    others.push(("a Chrome site answer", scope));
    let mut scope = proposal.scope.clone();
    let grant_id = aura_domain_common::PermissionGrantId::new();
    scope.lifetime = AuthorizationLifetime::Session { session_id };
    scope.network_consent = Some(NetworkSessionConsentRef {
        version: aura_domain_common::ToolWireVersionV1,
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        grant_id,
        grant_revision: Revision::INITIAL,
        workspace_id: call.snapshot.workspace_id,
        session_id,
        principal_id,
        caller: NetworkConsentCaller::OrdinaryTools,
        destinations: NetworkConsentDestinations::AllOrdinaryDestinations,
        approved_operation_fingerprint: call.op.fingerprint.clone(),
        revoked_grant_revision: None,
    });
    others.push(("a network answer", scope));
    let mut scope = proposal.scope.clone();
    scope.capability_limit = Some(vec![C::DesktopInput]);
    others.push(("a composite answer", scope));
    for (what, scope) in others {
        let mut answered = next(&call, act(PermissionMode::FullAccess, NOTEPAD, false));
        grant(&mut answered, scope);
        assert_eq!(asked(&answered), ANSWERS, "{what}");
    }
    // An answer for exactly this call, with no consent, is that call's own.
    let mut exact = next(&call, act(PermissionMode::FullAccess, NOTEPAD, false));
    grant(&mut exact, proposal.scope);
    assert!(allowed(&exact), "an exact answer");

    let mut answered = act(PermissionMode::ReadOnly, NOTEPAD, false);
    app_answer(&mut answered, NOTEPAD);
    let chrome = next(&answered, chrome_click());
    assert!(matches!(decide(&chrome), PolicyAuthorization::Ask { .. }), "Chrome after it");
    let mut file_write = next(&answered, Case::new(false));
    file_write.snapshot.mode = PermissionMode::ReadOnly;
    file_write.op.content.capabilities = vec![C::WorkspaceFileWrite];
    file_write.refresh();
    assert!(!allowed(&file_write), "a file write after it: {:?}", decide(&file_write));
}

#[test]
fn a_wrong_shape_is_not_an_operation() {
    let bad = |c: Case| {
        assert_eq!(denied(&c), PolicyDenyReason::InvalidOperation, "{}", c.op.content.action_kind)
    };
    // An operation without capabilities or resources cannot be made at all.
    for mode in MODES {
        bad(computer(mode, "computer.act", vec![C::DesktopInput], vec![app("", true)]));
        bad(computer(
            mode,
            "computer.act",
            vec![C::DesktopInput],
            vec![app(NOTEPAD, true), app(PAINT, true)],
        ));
        bad(computer(
            mode,
            "computer.act",
            vec![C::DesktopInput],
            vec![ResourceDescriptor::ChromeTab { tab_ref: "t1".into() }],
        ));
        bad(computer(
            mode,
            "computer.act",
            vec![C::DesktopInput, C::WorkspaceFileWrite],
            vec![app(NOTEPAD, true)],
        ));
        bad(computer(
            mode,
            "computer.act",
            vec![C::DesktopInput],
            vec![app(NOTEPAD, true), ResourceDescriptor::ChromeTab { tab_ref: "t1".into() }],
        ));
        // Listing the programs reads Aura's own catalog; it never carries a program's authority.
        bad(computer(mode, "computer.apps", vec![C::DesktopRead], vec![app(NOTEPAD, true)]));
        let mut networked = act(mode, NOTEPAD, true);
        networked.op.content.network = OperationNetwork::BrowserSitePolicy;
        networked.refresh();
        bad(networked);
        // Another tool may not borrow a computer-control capability or name a program.
        let mut borrowed = Case::new(false);
        borrowed.snapshot.mode = mode;
        borrowed.op.content.capabilities = vec![C::DesktopRead];
        borrowed.op.content.resources = vec![app(NOTEPAD, true)];
        borrowed.refresh();
        assert_eq!(denied(&borrowed), PolicyDenyReason::InvalidOperation, "another tool");
        let mut named = Case::new(false);
        named.snapshot.mode = mode;
        named.op.content.resources.push(app(NOTEPAD, true));
        named.refresh();
        assert_eq!(denied(&named), PolicyDenyReason::InvalidOperation, "a program named");
        let mut capability_alone = Case::new(false);
        capability_alone.snapshot.mode = mode;
        capability_alone.op.content.capabilities = vec![C::DesktopInput];
        capability_alone.refresh();
        assert_eq!(
            denied(&capability_alone),
            PolicyDenyReason::InvalidOperation,
            "a capability borrowed without a program"
        );
    }
}

#[test]
fn a_child_agent_cannot_use_a_program() {
    for mode in MODES {
        let mut c = read(mode, NOTEPAD, true);
        c.op.content.identity.actor = InvocationActor::ChildAgent;
        c.op.content.identity.parent_agent_run_id = Some(aura_domain_common::AgentRunId::new());
        c.refresh();
        assert_eq!(denied(&c), PolicyDenyReason::ModeDenied, "{mode:?}");
    }
}

/// Listing the programs reads Aura's own catalog as task state: allowed in every mode, like any
/// task read.
#[test]
fn listing_the_programs_is_a_task_read() {
    for mode in MODES {
        let mut listing =
            computer(mode, "computer.apps", vec![C::AgentTaskRead], vec![app(NOTEPAD, false)]);
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
