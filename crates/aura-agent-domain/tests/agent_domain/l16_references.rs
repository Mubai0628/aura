//! What a message refers to with "@": the composer's form is checked as it arrives, and the
//! resolved form is written after the message's text the same way every time.
use aura_agent_domain::*;
use aura_domain_common::WorkspacePath;

fn path(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("workspace path")
}

#[test]
fn a_reference_names_a_workspace_path_and_nothing_outside_it() {
    let parsed: AgentMessageReference =
        serde_json::from_value(serde_json::json!({"kind": "workspace_path", "path": "src/lib.rs"}))
            .expect("reference");
    assert_eq!(parsed, AgentMessageReference::WorkspacePath { path: path("src/lib.rs") });
    for outside in ["../secret.txt", "C:\\Windows\\win.ini", "", "src/../../x"] {
        let refused = serde_json::from_value::<AgentMessageReference>(
            serde_json::json!({"kind": "workspace_path", "path": outside}),
        );
        assert!(refused.is_err(), "{outside} is not a workspace path");
    }
    let unknown = serde_json::from_value::<AgentMessageReference>(
        serde_json::json!({"kind": "workspace_path", "path": "a.rs", "contents": "x"}),
    );
    assert!(unknown.is_err(), "a reference carries no contents");
}

#[test]
fn the_message_is_its_text_then_one_line_per_reference_in_order() {
    let references = [
        ResolvedMessageReference::WorkspaceFile { path: path("src/lib.rs") },
        ResolvedMessageReference::WorkspaceFolder { path: path("docs") },
        ResolvedMessageReference::WorkspaceFolder { path: path(".") },
    ];
    let whole = message_with_references("看一下这两处", &references);
    assert_eq!(
        whole,
        "看一下这两处\n\nReferred to in this message:\n\
         - workspace file `src/lib.rs`, not included: read it from the workspace if the task needs it\n\
         - workspace folder `docs`, not included: list or search it if the task needs it\n\
         - the workspace root folder `.`, not included: list or search it if the task needs it"
    );
    assert_eq!(whole, message_with_references("看一下这两处", &references), "the same bytes");
    assert_eq!(message_with_references("only text", &[]), "only text");
    assert!(
        message_with_references(" ", &references[..1]).starts_with("Referred to in this message:"),
        "a message of references alone starts with them"
    );
}

#[test]
fn the_conversation_shows_a_reference_by_its_kind_and_path() {
    let file = ResolvedMessageReference::WorkspaceFile { path: path("src/lib.rs") };
    let folder = ResolvedMessageReference::WorkspaceFolder { path: path("docs") };
    assert_eq!(
        file.view(),
        MessageReferenceView { kind: MessageReferenceKind::File, label: "src/lib.rs".into() }
    );
    assert_eq!(
        folder.view(),
        MessageReferenceView { kind: MessageReferenceKind::Folder, label: "docs".into() }
    );
    assert_eq!(MAX_MESSAGE_REFERENCES, 20);
}

#[test]
fn another_conversation_is_named_by_its_handle_with_its_recent_state_and_how_to_read_more() {
    let id: aura_domain_common::SessionId =
        serde_json::from_value(serde_json::json!("ses_01ARZ3NDEKTSV4RRFFQ69G5FAV")).expect("id");
    let handle = aura_domain_common::ConversationHandle::of(id);
    let parsed: AgentMessageReference = serde_json::from_value(
        serde_json::json!({"kind": "session", "session_id": "ses_01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
    )
    .expect("reference");
    assert_eq!(parsed, AgentMessageReference::Session { session_id: id });
    let earlier = ResolvedMessageReference::Session {
        conversation: handle.clone(),
        title: "整理发布清单".into(),
        recap: "T1 (2026-11-01T00:00:00Z): 列出发布前要做的事\nLast reply: 已列出七项".into(),
    };
    assert_eq!(
        message_with_references("接着这个做", std::slice::from_ref(&earlier)),
        format!(
            "接着这个做\n\nReferred to in this message:\n\
             - conversation \"整理发布清单\" (`{handle}`), not included: read more of it with \
             history.list_turns, history.search and history.read, passing \"conversation\": \
             \"{handle}\", if the task needs more than its recent state:\n  \
             > T1 (2026-11-01T00:00:00Z): 列出发布前要做的事\n  \
             > Last reply: 已列出七项"
        )
    );
    let empty = ResolvedMessageReference::Session {
        conversation: handle.clone(),
        title: "新会话".into(),
        recap: String::new(),
    };
    assert_eq!(
        message_with_references("", std::slice::from_ref(&empty)),
        format!(
            "Referred to in this message:\n- conversation \"新会话\" (`{handle}`), which has no \
             messages yet"
        )
    );
    assert_eq!(
        earlier.view(),
        MessageReferenceView {
            kind: MessageReferenceKind::Session, label: "整理发布清单".into()
        }
    );
    assert_eq!(earlier.conversation(), Some(&handle));
}

#[test]
fn a_receipt_keeps_another_conversation_by_its_handle_never_its_id() {
    let id = aura_domain_common::SessionId::new();
    let mut request = AgentSubmitRequest::new(
        aura_domain_common::SessionId::new(),
        "接着做",
        AgentInputLane::NextTurn,
    );
    request.references = vec![
        AgentMessageReference::Session { session_id: id },
        AgentMessageReference::WorkspacePath { path: path("src/lib.rs") },
    ];
    let receipt = request.receipt();
    assert_eq!(
        receipt.references,
        vec![
            AgentMessageReference::Conversation {
                conversation: aura_domain_common::ConversationHandle::of(id)
            },
            AgentMessageReference::WorkspacePath { path: path("src/lib.rs") },
        ]
    );
    assert_eq!(receipt, request.receipt(), "the same receipt every time");
    let kept = serde_json::to_string(&receipt).expect("json");
    assert!(!kept.contains(&id.to_string()), "{kept}");
}
