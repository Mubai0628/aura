//! `commands.*` through the production admission, policy and file path. Only the
//! model's calls are scripted; the commands live in the fixture workspace and its own
//! configuration directory.
use super::*;
use aura_security_contract::{ApprovalState, AuthorizationApproval};

const TOOLS: &[&str] = &["commands.list", "commands.describe", "commands.save", "commands.delete"];

fn seeded(root: &std::path::Path, name: &str) {
    let directory = root.join(".aura").join("commands").join(name);
    std::fs::create_dir_all(&directory).expect("seeded command");
    std::fs::write(
        directory.join("command.toml"),
        format!("name = \"{name}\"\ndescription = \"已有命令\"\nversion = 3\n"),
    )
    .expect("manifest");
    std::fs::write(directory.join("template.md"), "旧模板 $ARGUMENTS\n").expect("template");
}

fn save(name: &str, expected: serde_json::Value) -> serde_json::Value {
    json!({"name": name, "title": "审查改动", "description": "列出本次改动的风险",
           "argument_hint": "范围（可选）", "template": "请审查 $ARGUMENTS 的改动，列出风险。",
           "expected": expected})
}

/// Approves exactly this call once, the way the approval card does; asserts it is the only
/// answer offered.
fn approve_once(f: &Fixture, call: &CallContext) {
    let engine = f.service().agent_execution_required();
    let backend = engine.broker_backend(call).expect("file backend").descriptor();
    let PolicyAuthorization::Ask { proposal } = aura_permission_engine::policy::authorize(
        &call.policy(UtcTimestamp::now(), engine.runtime),
        &backend,
        None,
    ) else {
        panic!("a command change always asks")
    };
    assert_eq!(
        proposal.allowed_decisions,
        [ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );
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
}

/// Runs a call that needs no answer, continues the step, and returns the call.
async fn run(f: &mut Fixture, name: &str, input: serde_json::Value) -> ToolCallId {
    let batch = f.admit(&[(name, input)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    id
}

/// Runs a change after answering "allow once", continues the step, and returns the call.
async fn run_approved(f: &mut Fixture, name: &str, input: serde_json::Value) -> ToolCallId {
    let batch = f.admit(&[(name, input)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    approve_once(f, batch.calls.values().next().expect("an admitted change"));
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    id
}

fn refusal(f: &Fixture, call: ToolCallId) -> String {
    let store = f.service().broker.lock().expect("journal");
    let record = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
        .expect("recorded");
    match record.result.expect("terminal").outcome {
        ToolResultOutcomeV1::NotExecuted { error: Some(error), .. }
        | ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error } => error.message,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[tokio::test]
async fn a_save_asks_even_in_full_access_and_writes_exactly_the_previewed_files() {
    let mut f = Fixture::with_exposure(PermissionMode::FullAccess, TOOLS).await;
    let batch = f.admit(&[("commands.save", save("review", json!({"kind":"absent"})))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let call = batch.calls.values().next().expect("admitted").clone();
    let previews = call.operation.content.action["files"].as_array().expect("bound files").clone();
    assert_eq!(previews[0]["path"], ".aura/commands/review/command.toml");
    assert_eq!(previews[1]["content"], "请审查 $ARGUMENTS 的改动，列出风险。");
    assert!(!f.root.join(".aura").exists(), "nothing before the user's answer");
    // FullAccess still asks, with allow once or deny only.
    approve_once(&f, &call);
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    assert_eq!(f.output(id)["created"], true);
    let folder = f.root.join(".aura/commands/review");
    let manifest = std::fs::read_to_string(folder.join("command.toml")).expect("written");
    assert_eq!(manifest, previews[0]["content"].as_str().expect("preview"));
    assert!(manifest.contains("version = 1"));
    let call = run(&mut f, "commands.list", json!({})).await;
    let listed = f.output(call);
    let review = listed["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .find(|c| c["name"] == "review")
        .expect("listed")
        .clone();
    assert_eq!(review["workspace"], true);
    assert_eq!(review["issue"], serde_json::Value::Null);

    // Replacing needs the revision and a new answer; the first one is not reused.
    let stale = run(&mut f, "commands.save", save("review", json!({"kind":"absent"}))).await;
    assert!(refusal(&f, stale).contains("already exists"));
    let call = run(&mut f, "commands.describe", json!({"name":"review"})).await;
    let described = f.output(call);
    let mut changed = save("review", json!({"kind":"revision","revision":described["revision"]}));
    changed["template"] = "新的模板 $ARGUMENTS".into();
    // The first answer covered only that save; this one asks again.
    let id = run_approved(&mut f, "commands.save", changed).await;
    assert_eq!(f.output(id)["created"], false);
    assert_eq!(
        std::fs::read_to_string(folder.join("template.md")).expect("replaced"),
        "新的模板 $ARGUMENTS"
    );
    assert!(
        std::fs::read_to_string(folder.join("command.toml"))
            .expect("manifest")
            .contains("version = 2")
    );
}

#[tokio::test]
async fn an_unusable_definition_is_refused_with_every_reason_before_anyone_is_asked() {
    let mut f = Fixture::with_exposure(PermissionMode::FullAccess, TOOLS).await;
    let mut input = save("init", json!({"kind":"revision","revision":"x"}));
    input["template"] =
        "用 ghp_a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8 发布，见 [别处](../x.md)".into();
    let call = run(&mut f, "commands.save", input).await;
    let message = refusal(&f, call);
    assert!(message.starts_with("commands.refused"), "{message}");
    assert!(message.contains("is a built-in command"), "{message}");
    assert!(message.contains("像是包含凭据"), "{message}");
    let mut input = save("Bad_Name", json!({"kind":"absent"}));
    input["description"] = "".into();
    let call = run(&mut f, "commands.save", input).await;
    let message = refusal(&f, call);
    assert!(message.contains("use lowercase letters"), "{message}");
    assert!(!f.root.join(".aura").exists());
}

#[tokio::test]
async fn a_removal_asks_and_takes_the_folder_only_at_the_revision_read() {
    let mut f =
        Fixture::with_exposure_setup(PermissionMode::FullAccess, TOOLS, |root| seeded(root, "old"))
            .await;
    let call = run(&mut f, "commands.describe", json!({"name":"old"})).await;
    let described = f.output(call);
    assert_eq!(described["template"], "旧模板 $ARGUMENTS\n");
    assert!(described["manifest"].as_str().expect("manifest").contains("version = 3"));
    let stale = run(&mut f, "commands.delete", json!({"name":"old","revision":"not-it"})).await;
    assert!(refusal(&f, stale).contains("changed since you read it"));
    let id = run_approved(
        &mut f,
        "commands.delete",
        json!({"name":"old","revision":described["revision"]}),
    )
    .await;
    assert_eq!(f.output(id)["removed"], ".aura/commands/old");
    assert!(!f.root.join(".aura/commands/old").exists());
}

#[tokio::test]
async fn the_users_own_commands_and_linked_folders_stay_out_of_reach() {
    let elsewhere = tempfile::tempdir().expect("outside the workspace");
    let outside = elsewhere.path().to_path_buf();
    let mut f = Fixture::with_exposure_setup(PermissionMode::FullAccess, TOOLS, |root| {
        std::fs::create_dir_all(outside.join("commands")).expect("outside folder");
        std::fs::create_dir(root.join(".aura")).expect(".aura");
        junction::create(outside.join("commands"), root.join(".aura").join("commands"))
            .expect("a linked commands folder");
    })
    .await;
    let mine = f._host.root.path().join("config").join("commands").join("mine");
    std::fs::create_dir_all(&mine).expect("the user's own command");
    std::fs::write(
        mine.join("command.toml"),
        "name = \"mine\"\ndescription = \"我的\"\nversion = 1\n",
    )
    .expect("manifest");
    std::fs::write(mine.join("template.md"), "私人模板").expect("template");
    let call = run(&mut f, "commands.list", json!({})).await;
    let listed = f.output(call);
    assert!(
        listed["commands"].as_array().expect("commands").iter().all(|c| c["workspace"] == false),
        "neither the user's commands nor a linked folder's are listed: {listed}"
    );
    assert!(!listed.to_string().contains("私人模板"));
    let call = run(&mut f, "commands.save", save("review", json!({"kind":"absent"}))).await;
    let message = refusal(&f, call);
    assert!(message.starts_with("commands.protected_target"), "{message}");
    assert!(
        std::fs::read_dir(outside.join("commands")).expect("outside").next().is_none(),
        "nothing was written through the link"
    );
}
