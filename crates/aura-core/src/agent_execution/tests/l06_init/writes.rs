//! What `/init` writes: the fixed structure, existing files by their hash, guides in new
//! folders, and in ReadOnly exactly the previewed bytes after one approval.
use super::*;
use aura_security_contract::{ApprovalState, AuthorizationApproval};

#[tokio::test]
async fn a_new_agents_file_is_written_without_asking_in_workspace_mode() {
    let mut f = init_fixture(PermissionMode::WorkspaceReadWrite).await;
    let id = run(&mut f, submission()).await;
    let output = f.output(id);
    assert_eq!(output["disposition"], "applied");
    assert_eq!(output["files"], json!([{"path":"AGENTS.md","created":true}]));
    let written = std::fs::read_to_string(f.root.join("AGENTS.md")).expect("written");
    assert!(aura_workspace_domain::standard_structure(&written), "{written}");
    assert!(written.contains("- `cargo test --workspace` — Run every Rust test"));
    assert!(written.contains("Source: defined in `README.md`"));
}

#[tokio::test]
async fn an_existing_file_is_replaced_only_through_its_hash_and_keeps_the_structure() {
    let mut f = init_fixture(PermissionMode::FullAccess).await;
    std::fs::write(f.root.join("AGENTS.md"), "# Notes\n\nOld free-form rules.\r\n")
        .expect("a non-standard file the user wrote");
    let mut input = submission();
    input["expected"] = json!({"expected_kind":"file_hash","hash":logical_hash(&f, "AGENTS.md")});
    let id = run(&mut f, input).await;
    assert_eq!(f.output(id)["files"], json!([{"path":"AGENTS.md","created":false}]));
    let written = std::fs::read_to_string(f.root.join("AGENTS.md")).expect("migrated");
    assert!(aura_workspace_domain::standard_structure(&written.replace("\r\n", "\n")));
    assert!(!written.contains("Old free-form rules"), "the model chose what to keep");
}

#[tokio::test]
async fn guides_are_created_in_new_folders_before_the_root_file_links_them() {
    let mut f = init_fixture(PermissionMode::WorkspaceReadWrite).await;
    let mut input = submission();
    input["testing"] = "See the [testing guide](docs/agent-guides/testing.md).".into();
    input["guides"] = json!([{
        "name": "testing", "title": "Testing", "body": "## Layout\n\nTests sit beside the code.",
        "expected": {"expected_kind":"absent"}
    }]);
    let id = run(&mut f, input).await;
    assert_eq!(
        f.output(id)["files"],
        json!([
            {"path":"docs/agent-guides/testing.md","created":true},
            {"path":"AGENTS.md","created":true}
        ])
    );
    let guide = std::fs::read_to_string(f.root.join("docs/agent-guides/testing.md"))
        .expect("the guide and its folders were created");
    assert_eq!(guide, "# Testing\n\n## Layout\n\nTests sit beside the code.\n");
}

#[tokio::test]
async fn readonly_offers_one_answer_and_writes_exactly_the_previewed_bytes() {
    let f = init_fixture(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[("workspace_init.complete", submission())]);
    let id = batch.batch.calls[0].tool_call_id;
    let call = batch.calls.values().next().expect("the init call was admitted").clone();
    let engine = f.service().agent_execution_required();
    let backend = engine.broker_backend(&call).expect("file backend").descriptor();
    let PolicyAuthorization::Ask { proposal } = aura_permission_engine::policy::authorize(
        &call.policy(UtcTimestamp::now(), engine.runtime),
        &backend,
        None,
    ) else {
        panic!("ReadOnly asks before /init writes")
    };
    assert_eq!(
        proposal.allowed_decisions,
        [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );
    let previews = call.operation.content.action["files"].as_array().expect("bound files").clone();
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0]["path"], "AGENTS.md");
    assert_eq!(previews[0]["previous"], serde_json::Value::Null);
    assert!(std::fs::metadata(f.root.join("AGENTS.md")).is_err(), "nothing before approval");
    let now = UtcTimestamp::now();
    let approval = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &approval)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                approval.approval_id,
                approval.revision,
                &call.operation,
                ApprovalDecisionKind::AllowOnce,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("approve once");
    f.execute(batch).await;
    assert_eq!(f.output(id)["files"][0]["created"], true);
    let written =
        std::fs::read_to_string(f.root.join("AGENTS.md")).expect("written after approval");
    assert_eq!(written, previews[0]["content"].as_str().expect("preview text"));
}
