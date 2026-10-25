use super::*;
use aura_domain_common::{AgentRunId, NetworkGrantId, TurnTaskId, UtcTimestamp, WorkspaceId};
use aura_permission_domain::*;
use aura_security_contract::{
    ApprovalDecisionKind, InvocationIdentity, NetworkDataClass, NetworkDestinationClass,
    PermissionGrantLifetime, PermissionMode, ResolvedCapability,
};
use aura_tool_contract::ToolRegistryScopeV1;

fn network_grant(
    state: &RuntimeState,
    w: &WorkspaceSummary,
    s: SessionId,
    port: u16,
) -> NetworkGrantId {
    let store = &services(state).persistence;
    store
        .ensure_session_permission_state(
            s,
            w.workspace_id,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("permission");
    let snapshot =
        store.create_permission_snapshot(s, None, CorrelationId::new()).expect("snapshot");
    let approval = store
        .create_approval_request(
            ApprovalRequestDraft {
                identity: InvocationIdentity::runtime_user(w.workspace_id, s, CorrelationId::new()),
                operation_ids: vec![aura_domain_common::SecurityOperationId::new()],
                operation_fingerprints: vec![ContentHash::blake3(b"test HTTP read")],
                title: "Local HTTP fixture".into(),
                model_purpose: None,
                actual_operation_summary: "GET fixture".into(),
                source_summary: "test user".into(),
                source: SecuritySource::UserInterface,
                source_fingerprint: None,
                capabilities: vec![ResolvedCapability::PublicNetworkRead],
                scopes: vec![ResolvedScope::NetworkOrigin {
                    scheme: "http".into(),
                    host: "127.0.0.1".into(),
                    port,
                }],
                risk_flags: vec![],
                allowed_decisions: vec![
                    ApprovalDecisionKind::AllowSession,
                    ApprovalDecisionKind::Deny,
                ],
                permission_snapshot_id: snapshot.id,
                release_descriptor: None,
            },
            CorrelationId::new(),
        )
        .expect("actual approval");
    let commit = store
        .resolve_approval(
            approval.id,
            approval.request_revision,
            ApprovalDecisionKind::AllowSession,
            CorrelationId::new(),
        )
        .expect("approve once");
    let id = NetworkGrantId::new();
    store
        .create_network_grant(
            NetworkGrant {
                id,
                identity_scope: NetworkGrantIdentityScope::Session,
                identity_scope_id: s.to_string(),
                lifetime: PermissionGrantLifetime::Session,
                origins: vec![NormalizedOrigin {
                    scheme: "http".into(),
                    host: "127.0.0.1".into(),
                    port,
                }],
                destination_classes: vec![NetworkDestinationClass::Loopback],
                methods: vec!["GET".into()],
                allowed_data_classes: vec![NetworkDataClass::PublicQuery],
                max_upload_bytes: Some(0),
                max_download_bytes: Some(1024),
                source_fingerprint: None,
                expires_at: None,
                revoked_at: None,
            },
            commit.resolution.decision_id,
            CorrelationId::new(),
        )
        .expect("grant linked to decision");
    id
}

#[tokio::test]
async fn r05_http_real_gateway_enforces_origin_revocation_redirect_and_output_limit() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (_, w, s) = workspace(&fixture, &state).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("local server");
    let port = listener.local_addr().expect("address").port();
    let grant = network_grant(&state, &w, s, port);
    let server = tokio::spawn(async move {
        for response in [
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello",
            "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 5\r\nConnection: close\r\n\r\nlarge",
        ] {
            let (mut stream, _) = listener.accept().await.expect("request");
            let mut bytes = [0; 4096];
            let n = stream.read(&mut bytes).await.expect("request bytes");
            let text = String::from_utf8_lossy(&bytes[..n]).to_lowercase();
            assert!(text.starts_with("get /"));
            assert!(!text.contains("authorization:"));
            assert!(!text.contains("cookie:"));
            stream.write_all(response.as_bytes()).await.expect("response");
        }
    });
    let input = |url: String, max_bytes: u32| json!({"workspace_id":w.workspace_id,"session_id":s,"url":url,"network_grant_id":grant,"max_bytes":max_bytes});
    let url = format!("http://127.0.0.1:{port}/read");
    let response = success(&state, "broker.http_read", input(url.clone(), 100)).await;
    assert_eq!(response["body_base64"], "aGVsbG8=");
    assert!(
        request(&state, "broker.http_read", input(url.clone(), 100)).await.get("error").is_some()
    );
    assert!(
        request(&state, "broker.http_read", input(url.clone(), 2)).await.get("error").is_some()
    );
    tokio::time::timeout(std::time::Duration::from_secs(3), server)
        .await
        .expect("server finished")
        .expect("server task");
    assert!(
        request(&state, "broker.http_read", input("http://127.0.0.1:1/escape".into(), 100))
            .await
            .get("error")
            .is_some()
    );
    services(&state).persistence.revoke_network_grant(grant, CorrelationId::new()).expect("revoke");
    assert!(request(&state, "broker.http_read", input(url, 100)).await.get("error").is_some());
    assert_eq!(results(&state).len(), 3);
    assert!(results(&state).iter().all(|r| r.side_effect == SideEffectStateV1::NoEffect));
}

#[tokio::test]
async fn r05_builtin_registry_and_context_use_persisted_services_without_packages() {
    use aura_tool_contract::*;
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, s) = workspace(&fixture, &state).await;
    let store = &services(&state).persistence;
    let committed = store
        .build_and_commit_tool_registry_snapshot(aura_persistence::ToolRegistryBuildRequestV1 {
            scope: ToolRegistryScopeV1::Workspace { workspace_id: w.workspace_id },
            settings: services(&state)._settings.tool_settings_snapshot().expect("settings"),
            mode_permission_context_hash: aura_domain_common::jcs::canonical_sha256(
                &Option::<u8>::None,
            )
            .expect("hash"),
            core_exposed_tool_ids: Default::default(),
            non_package_candidates: vec![],
            created_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
        })
        .expect("actual builtin registry");
    let corpus = store
        .read_tool_registry_search_corpus(&committed.registry_snapshot_ref)
        .expect("read")
        .expect("published");
    assert!(
        !corpus.snapshot.catalog_entries.is_empty(),
        "signed builtins must materialize without Package installation"
    );
    success(&state,"broker.tools",json!({"registry_snapshot_ref":committed.registry_snapshot_ref,"action":{"kind":"list_loaded"}})).await;
    let agent = AgentRunId::new();
    let status = success(
        &state,
        "broker.context",
        json!({"kind":"status","request":{"session_id":s,"agent_run_id":agent}}),
    )
    .await;
    assert_eq!(status["value"]["session_id"], json!(s));
    assert!(
        request(
            &state,
            "broker.context",
            json!({"kind":"status","request":{"session_id":SessionId::new(),"agent_run_id":agent}})
        )
        .await
        .get("error")
        .is_some()
    );
    // Project Vault content uses the same secure filesystem service. No second
    // runtime Memory database or migration of the user's Global Vault exists.
    fs::create_dir(root.join(".memories")).expect("vault");
    success(
        &state,
        method::FS_WRITE_FILE,
        write_input(&w, s, ".memories/project.md", "# Project\nremembered"),
    )
    .await;
    assert_eq!(
        fs::read_to_string(root.join(".memories/project.md")).expect("Obsidian file"),
        "# Project\nremembered"
    );
}

#[tokio::test]
async fn a_normal_session_pin_lands_in_that_sessions_effective_registry_only() {
    // The positive half of the "trust is not loading" rule, through the production
    // commands only: a normal Session pin must make the Tool visible in *that*
    // Session's Effective Registry with its load source, and must not appear in
    // another Session of the same Workspace. No UI entry is needed or added.
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let services = services(&state);
    let root = fixture.root.path().join("pin-scope");
    std::fs::create_dir_all(&root).expect("workspace root");
    let workspace_id = WorkspaceId::new();
    let first = (SessionId::new(), TurnTaskId::new(), AgentRunId::new());
    let second = (SessionId::new(), TurnTaskId::new(), AgentRunId::new());
    let scopes = [first, second].map(|(session_id, turn_task_id, agent_run_id)| {
        crate::p07_test_support::seed_session_scope(
            &services.persistence,
            &fixture.paths.database_path,
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
            &root,
        )
        .expect("seed a Session scope with a running TurnTask and AgentRun");
        ToolRegistryScopeV1::Session { workspace_id, session_id, turn_task_id, agent_run_id }
    });
    for scope in &scopes {
        let published = crate::p07_registry::ToolRegistryControlServiceV1
            .build_scope(
                &services.persistence,
                crate::p07_test_support::registry_request(scope.clone()),
            )
            .expect("initial Session Registry head");
        assert!(published.projection_publish_error.is_none());
    }
    let head = services
        .persistence
        .read_tool_registry_scope_head(&scopes[0])
        .expect("read the head")
        .expect("the Session has a head");
    let corpus = services
        .persistence
        .read_tool_registry_search_corpus(&head)
        .expect("read the corpus")
        .expect("the corpus exists");
    // A Built-in from the real signed release. The five core-exposed Tool definitions
    // are effective without any load, so what a normal pin adds for one of them is its
    // *load source*: the effective entry is the same Tool, now pinned in exactly this
    // Session.
    let entry = corpus.snapshot.catalog_entries.first().expect("a Built-in catalog entry").clone();

    let effective = |session_id: SessionId| {
        json!({
            "projection_kind": "session_effective_tool_registry",
            "projection_scope": { "type": "session", "session_id": session_id },
        })
    };
    let pin_state_of = |snapshot: &serde_json::Value| -> serde_json::Value {
        // `projection.snapshot` wraps the payload under `payload`.
        let payload = snapshot.get("payload").unwrap_or(snapshot);
        payload["tools"]
            .as_array()
            .unwrap_or_else(|| panic!("effective Tools: {snapshot}"))
            .iter()
            .find(|tool| tool["tool_id"] == serde_json::to_value(entry.tool_id).expect("tool id"))
            .map(|tool| tool["pin_state"].clone())
            .unwrap_or_else(|| panic!("the Tool has to be in the effective registry: {snapshot}"))
    };
    assert!(
        pin_state_of(&success(&state, method::PROJECTION_SNAPSHOT, effective(first.0)).await)
            .is_null(),
        "no load source before the pin"
    );
    assert!(
        pin_state_of(&success(&state, method::PROJECTION_SNAPSHOT, effective(second.0)).await)
            .is_null(),
        "no load source in the other Session either"
    );

    success(
        &state,
        "tools.pin.session.set",
        json!({
            "session_id": first.0,
            "tool_id": entry.tool_id,
            "tool_version": entry.tool_version,
            "expected_entity_revision": null,
        }),
    )
    .await;

    let pinned = success(&state, method::PROJECTION_SNAPSHOT, effective(first.0)).await;
    let source = pin_state_of(&pinned);
    assert!(
        !source.is_null(),
        "the effective entry has to show where the Tool came from: {pinned}"
    );

    let other = success(&state, method::PROJECTION_SNAPSHOT, effective(second.0)).await;
    assert!(
        pin_state_of(&other).is_null(),
        "another Session of the same Workspace must not see this Session's pin: {other}"
    );
}

#[tokio::test]
async fn r05_registry_search_describe_load_unload_preserve_exact_versions() {
    use aura_tool_contract::*;
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (_, w, s) = workspace(&fixture, &state).await;
    let store = &services(&state).persistence;
    let turn = aura_domain_common::TurnTaskId::new();
    let agent = AgentRunId::new();
    let at = UtcTimestamp::now().to_string();
    // Persisted pre-existing turn fixture, not a Driver or production loop.
    let mut db =
        rusqlite::Connection::open(&fixture.paths.database_path).expect("isolated domain DB");
    db.execute_batch("PRAGMA foreign_keys=ON").expect("FK");
    let tx = db.transaction().expect("fixture transaction");
    tx.execute(
        "INSERT INTO turn_tasks VALUES(?1,?2,'running',?3,?3)",
        rusqlite::params![turn.to_string(), s.to_string(), at],
    )
    .expect("fixture turn");
    tx.execute(
        "INSERT INTO agent_runs VALUES(?1,?2,NULL,'running',?3,?3)",
        rusqlite::params![agent.to_string(), turn.to_string(), at],
    )
    .expect("fixture agent");
    tx.commit().expect("fixture commit");
    store
        .ensure_session_permission_state(
            s,
            w.workspace_id,
            PermissionMode::ReadOnly,
            CorrelationId::new(),
        )
        .expect("permission");
    let permission = store.session_permission_state(s).expect("permission");
    let committed = store
        .build_and_commit_tool_registry_snapshot(aura_persistence::ToolRegistryBuildRequestV1 {
            scope: ToolRegistryScopeV1::Session {
                workspace_id: w.workspace_id,
                session_id: s,
                turn_task_id: turn,
                agent_run_id: agent,
            },
            settings: services(&state)._settings.tool_settings_snapshot().expect("settings"),
            mode_permission_context_hash: aura_domain_common::jcs::canonical_sha256(&Some(
                permission,
            ))
            .expect("hash"),
            core_exposed_tool_ids: Default::default(),
            non_package_candidates: vec![],
            created_at: UtcTimestamp::now(),
            correlation_id: CorrelationId::new(),
        })
        .expect("registry");
    let source = store
        .read_tool_registry_search_corpus(&committed.registry_snapshot_ref)
        .expect("corpus")
        .expect("snapshot");
    let content = "Persisted R05 Context content".to_owned();
    let mut event = aura_persistence::PersistentEventDraft::system(
        "R05ContextFixture",
        CorrelationId::new(),
        json!({}),
    );
    event.workspace_id = Some(w.workspace_id);
    event.session_id = Some(s);
    event.turn_task_id = Some(turn);
    event.agent_run_id = Some(agent);
    event.searchable_text = Some(content.clone());
    let event_id = store
        .commit_context_source_event(aura_persistence::ContextSourceEventCommitRequest {
            event,
            canonical_content: content.clone(),
            model_visible: true,
            explicitly_user_authored: false,
        })
        .expect("persist Context source");
    // Seed the pre-existing P06 model-visible index alongside the persisted
    // source event. This tests production history reads, not R09 compilation.
    db.execute(
        "INSERT INTO context_search_chunks(session_id,agent_run_id,turn_task_id,source_type,source_id,session_sequence,privacy_class,source_hash_json,searchable_text,created_at)
         SELECT session_id,agent_run_id,turn_task_id,'event',event_id,session_sequence,'model_visible',?2,?3,created_at FROM events WHERE event_id=?1",
        rusqlite::params![event_id.to_string(), serde_json::to_string(&ContentHash::sha256(content.as_bytes())).expect("source hash"), content],
    ).expect("persist prior Context index");
    let read=success(&state,"broker.context",json!({"kind":"read_events","request":{"session_id":s,"agent_run_id":agent,"selectors":[{"type":"event","event_id":event_id}],"character_limit":1000,"cursor":null}})).await;
    assert_eq!(read["value"]["events"][0]["content"], content);
    let entry = source
        .snapshot
        .catalog_entries
        .iter()
        .find(|e| e.load_state == ToolLoadStateV1::Core)
        .expect("required Core builtin");
    let definition = ToolDefinitionSnapshotRef {
        tool_id: entry.tool_id,
        tool_version: entry.tool_version.clone(),
        registry_snapshot_ref: committed.registry_snapshot_ref.clone(),
    };
    success(&state,"broker.tools",json!({"registry_snapshot_ref":committed.registry_snapshot_ref,"action":{"kind":"search","request":{"query":"tools","limit":10,"cursor":null}}})).await;
    success(&state,"broker.tools",json!({"registry_snapshot_ref":committed.registry_snapshot_ref,"action":{"kind":"describe","request":{"definition_ref":definition}}})).await;
    let loaded=success(&state,"broker.tools",json!({"registry_snapshot_ref":committed.registry_snapshot_ref,"action":{"kind":"load","request":{"definitions":[definition]}}})).await;
    // Required Core definitions cannot be removed or ephemerally reloaded.
    assert_eq!(loaded["changed"], false);
    assert_eq!(
        results(&state).last().expect("load receipt").side_effect,
        SideEffectStateV1::NoEffect
    );
    let mut definition = definition;
    definition.registry_snapshot_ref =
        serde_json::from_value(loaded["resulting_registry_snapshot_ref"].clone())
            .expect("new exact snapshot");
    let noop=success(&state,"broker.tools",json!({"registry_snapshot_ref":definition.registry_snapshot_ref,"action":{"kind":"load","request":{"definitions":[definition]}}})).await;
    assert_eq!(noop["changed"], false);
    assert_eq!(
        results(&state).last().expect("no-op receipt").side_effect,
        SideEffectStateV1::NoEffect
    );
    let unloaded=success(&state,"broker.tools",json!({"registry_snapshot_ref":definition.registry_snapshot_ref,"action":{"kind":"unload","request":{"definitions":[definition]}}})).await;
    assert_eq!(unloaded["changed"], false);
    assert_eq!(
        results(&state).last().expect("unload receipt").side_effect,
        SideEffectStateV1::NoEffect
    );
    definition.registry_snapshot_ref.content_hash =
        aura_domain_common::jcs::CanonicalHashV1::sha256(b"wrong immutable snapshot");
    assert!(request(&state,"broker.tools",json!({"registry_snapshot_ref":committed.registry_snapshot_ref,"action":{"kind":"load","request":{"definitions":[definition]}}})).await.get("error").is_some());

    // Tool configuration writes use the same production command journal and
    // Broker receipt path, including Built-in pins without Package installation.
    let pin=success(&state,"tools.pin.workspace.set",json!({"workspace_id":w.workspace_id,"tool_id":entry.tool_id,"tool_version":entry.tool_version,"expected_entity_revision":null})).await;
    assert_eq!(
        results(&state).last().expect("pin receipt").side_effect,
        SideEffectStateV1::FullyApplied
    );
    let pins = store
        .tool_authority_snapshot(Some(w.workspace_id), Some(s), None)
        .expect("persisted pins")
        .workspace_pins;
    assert_eq!(pins.len(), 1);
    let revision = match &pins[0] {
        aura_tool_domain::ToolPinRecordV1::Workspace { entity_revision, .. } => *entity_revision,
        _ => panic!("workspace pin"),
    };
    assert!(pin.is_object());
    success(&state,"tools.pin.workspace.set",json!({"workspace_id":w.workspace_id,"tool_id":entry.tool_id,"tool_version":entry.tool_version,"expected_entity_revision":revision})).await;
    assert_eq!(
        results(&state).last().expect("pin no-op receipt").side_effect,
        SideEffectStateV1::NoEffect
    );
    success(&state,"tools.pin.workspace.clear",json!({"workspace_id":w.workspace_id,"tool_id":entry.tool_id,"expected_entity_revision":revision})).await;
    assert_eq!(
        results(&state).last().expect("clear receipt").side_effect,
        SideEffectStateV1::FullyApplied
    );
    assert!(
        store
            .tool_authority_snapshot(Some(w.workspace_id), Some(s), None)
            .expect("cleared pin")
            .workspace_pins
            .is_empty()
    );
}
