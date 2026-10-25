//! MCP tool calls: one coarse capability; WorkspaceReadWrite and
//! FullAccess allow, ReadOnly asks once per server per turn; a turn answer covers only that
//! server, configuration, session, turn and parent agent; nothing else may borrow the shape.
use super::*;
use aura_security_contract::McpTurnConsentRef;

/// A named change to a call's content, or to the whole case.
type Edit = (&'static str, Box<dyn Fn(&mut OperationContent)>);
type Change = (&'static str, Box<dyn Fn(&mut Case)>);

fn config(seed: char) -> ContentHash {
    ContentHash::new(aura_domain_common::ContentHashAlgorithm::Blake3, seed.to_string().repeat(64))
        .expect("hash")
}

fn mcp(mode: PermissionMode, server: &str, tool: &str) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.profile = SecurityProfile::for_permission_mode(mode);
    c.op.content.source = SecuritySourceRef::McpServer { server_id: server.into() };
    c.op.content.source_revision_fingerprint = Some(config('a'));
    c.op.content.action_kind = format!("mcp.{server}.{tool}");
    c.op.content.action = serde_json::json!({"text": "x"});
    c.op.content.capabilities = vec![ResolvedCapability::ExternalExtensionCall];
    c.op.content.network = OperationNetwork::None;
    c.op.content.resources = vec![ResourceDescriptor::McpServer {
        server_id: server.into(),
        config_fingerprint: config('a'),
    }];
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities = [ResolvedCapability::ExternalExtensionCall].into();
    // Core's own part of the call: it writes nothing and hands the server no credential.
    c.backend.guarantees =
        [EnforcementGuarantee::AuthorizedWritesOnly, EnforcementGuarantee::CredentialIsolation]
            .into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

/// The production resolution of an "allow for this turn" answer.
fn turn_answer(c: &mut Case) {
    let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
        panic!("read-only asks first")
    };
    assert_eq!(
        proposal.allowed_decisions,
        vec![ApprovalDecisionKind::AllowForCurrentTurn, ApprovalDecisionKind::Deny]
    );
    let identity = &c.op.content.identity;
    let (session_id, turn_task_id) =
        (identity.session_id.expect("session"), identity.turn_task_id.expect("turn"));
    let mut scope = proposal.scope;
    scope.lifetime = AuthorizationLifetime::TurnTask { session_id, turn_task_id };
    scope.mcp_consent = Some(McpTurnConsentRef {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        session_id,
        turn_task_id,
        principal_id: identity.principal_id,
        server_id: "files".into(),
        config_fingerprint: config('a'),
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        grant_revision: Revision::INITIAL,
    });
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

/// The next call in the case: `edit` changes its content, the grants stay.
fn next(c: &mut Case, edit: impl FnOnce(&mut OperationContent)) {
    let mut content = c.op.content.clone();
    edit(&mut content);
    c.op = SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), content)
        .expect("operation");
}

fn allowed(c: &Case) -> bool {
    matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. })
}

fn asks(c: &Case) -> bool {
    matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. })
}

fn denied(c: &Case) -> Option<PolicyDenyReason> {
    match authorize(&c.input(), &c.backend, None) {
        PolicyAuthorization::Deny { reason } => Some(reason),
        _ => None,
    }
}

#[test]
fn mcp_calls_follow_the_mode_matrix() {
    let full = mcp(PermissionMode::FullAccess, "files", "echo");
    assert!(allowed(&full), "{:?}", authorize(&full.input(), &full.backend, None));
    assert!(allowed(&mcp(PermissionMode::WorkspaceReadWrite, "files", "echo")));
    assert!(asks(&mcp(PermissionMode::ReadOnly, "files", "echo")));
}

#[test]
fn a_turn_answer_covers_the_server_in_that_turn_and_nothing_else() {
    let mut c = mcp(PermissionMode::ReadOnly, "files", "echo");
    turn_answer(&mut c);
    assert!(allowed(&c), "the approved call itself");
    next(&mut c, |content| {
        content.action_kind = "mcp.files.slow".into();
        content.action = serde_json::json!({"ms": 10});
    });
    assert!(allowed(&c), "another tool of the same server in the same turn");

    let base = c.op.content.clone();
    let cases: [Edit; 5] = [
        (
            "another turn",
            Box::new(|content| {
                content.identity.turn_task_id = Some(aura_domain_common::TurnTaskId::new())
            }),
        ),
        (
            "another server",
            Box::new(|content| {
                content.source = SecuritySourceRef::McpServer { server_id: "notes".into() };
                content.action_kind = "mcp.notes.echo".into();
                content.resources = vec![ResourceDescriptor::McpServer {
                    server_id: "notes".into(),
                    config_fingerprint: config('a'),
                }];
            }),
        ),
        (
            "a configuration trusted again after a change",
            Box::new(|content| {
                content.source_revision_fingerprint = Some(config('b'));
                content.resources = vec![ResourceDescriptor::McpServer {
                    server_id: "files".into(),
                    config_fingerprint: config('b'),
                }];
            }),
        ),
        (
            "another principal",
            Box::new(|content| {
                content.identity.principal_id = aura_domain_common::PrincipalId::new()
            }),
        ),
        (
            "a child agent",
            Box::new(|content| {
                content.identity.actor = InvocationActor::ChildAgent;
                content.identity.parent_agent_run_id = Some(aura_domain_common::AgentRunId::new());
            }),
        ),
    ];
    for (name, edit) in cases {
        c.op = SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), {
            let mut content = base.clone();
            edit(&mut content);
            content
        })
        .expect("operation");
        assert!(!allowed(&c), "{name} is not covered by the turn answer");
    }

    // A revoked answer covers nothing.
    c.op = SecuredOperation::new(aura_domain_common::SecurityOperationId::new(), base)
        .expect("operation");
    assert!(allowed(&c));
    c.grants[0].revoked_at = Some(c.snapshot.created_at);
    assert!(asks(&c));
}

#[test]
fn a_denial_in_the_turn_refuses_without_asking() {
    let c = mcp(PermissionMode::ReadOnly, "files", "echo");
    let mut input = c.input();
    input.denied_for_turn = true;
    assert!(matches!(
        authorize(&input, &c.backend, None),
        PolicyAuthorization::Deny { reason: PolicyDenyReason::DeniedForTurn }
    ));
}

#[test]
fn nothing_else_may_borrow_the_mcp_shape() {
    let shapes: [Change; 6] = [
        (
            "the MCP capability on a built-in tool",
            Box::new(|c| {
                c.op.content.source =
                    SecuritySourceRef::BuiltInTool { identity_hash: c.op.fingerprint.0.clone() };
            }),
        ),
        (
            "another server's action",
            Box::new(|c| c.op.content.action_kind = "mcp.notes.echo".into()),
        ),
        (
            "a second capability",
            Box::new(|c| c.op.content.capabilities.push(ResolvedCapability::WorkspaceFileRead)),
        ),
        ("a declared network", Box::new(|c| c.op.content.network = OperationNetwork::BroadProcess)),
        (
            "a second resource",
            Box::new(|c| {
                let workspace_id = c.snapshot.workspace_id;
                c.op.content.resources.push(ResourceDescriptor::WorkspaceRoot { workspace_id });
            }),
        ),
        ("no trusted configuration", Box::new(|c| c.op.content.source_revision_fingerprint = None)),
    ];
    for (name, change) in shapes {
        let mut c = mcp(PermissionMode::FullAccess, "files", "echo");
        change(&mut c);
        c.refresh();
        assert!(denied(&c).is_some(), "{name} is refused, even in FullAccess");
    }
}
