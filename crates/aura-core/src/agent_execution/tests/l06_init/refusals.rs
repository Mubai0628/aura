//! What `/init` refuses before anything is written or anyone is asked, and how a refusal
//! reads to the model.
use super::*;

fn shell(script: &str) -> serde_json::Value {
    json!({"script":script,"requires_network":false,"working_directory":".","stdin":"","timeout_ms":20000,"max_output_bytes":4096})
}

async fn ran(f: &mut Fixture, tool: &str, script: &str) -> ToolCallId {
    let batch = f.admit(&[(tool, shell(script))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    f.continue_after_tools(batch_id, &group);
    id
}

#[tokio::test]
async fn fabricated_or_circular_evidence_is_refused_with_every_reason_at_once() {
    let mut f = init_fixture(PermissionMode::FullAccess).await;
    let mut input = submission();
    input["commands"]["entries"] = json!([
        {"command":"cargo nextest run","purpose":"Run tests","shell":"powershell",
         "evidence":{"kind":"repository_defined","path":"README.md"}},
        {"command":COMMAND,"purpose":"Run tests","shell":"powershell",
         "evidence":{"kind":"repository_defined","path":"AGENTS.md"}},
        {"command":COMMAND,"purpose":"Run tests","shell":"powershell",
         "evidence":{"kind":"execution_verified","tool_call_id":ToolCallId::new()}}
    ]);
    let call = run(&mut f, input).await;
    let message = refusal(&f, call);
    assert!(message.starts_with("workspace_init.refused: 3 problem(s)"), "{message}");
    assert!(message.contains("README.md does not contain `cargo nextest run`"), "{message}");
    assert!(message.contains("AGENTS.md is what /init writes"), "{message}");
    assert!(message.contains("is not a shell.powershell or shell.cmd call"), "{message}");
    assert!(!f.root.join("AGENTS.md").exists());
}

#[tokio::test]
async fn an_executed_command_counts_only_after_it_succeeded_here_in_the_documented_shell() {
    let mut f = init_fixture(PermissionMode::WorkspaceReadWrite).await;
    f.service().commands_required().wait_until_ready().await.expect("managed runtime verified");
    let succeeded = ran(&mut f, "shell.powershell", "Write-Output 'aura-init-check'").await;
    let failed = ran(&mut f, "shell.powershell", "Write-Output 'aura-init-fail'; exit 3").await;
    let evidence = |call: ToolCallId, command: &str, shell: &str| {
        json!({"command":command,"purpose":"Print a marker","shell":shell,
               "evidence":{"kind":"execution_verified","tool_call_id":call}})
    };
    let mut input = submission();
    input["commands"]["entries"] = json!([
        evidence(failed, "Write-Output 'aura-init-fail'", "powershell"),
        evidence(succeeded, "Write-Output 'aura-init-check'", "cmd"),
    ]);
    let call = run(&mut f, input).await;
    let message = refusal(&f, call);
    assert!(message.contains("did not finish with exit code 0"), "{message}");
    assert!(message.contains("ran in a different shell than the one documented"), "{message}");
    let mut input = submission();
    input["commands"]["entries"] =
        json!([evidence(succeeded, "Write-Output 'aura-init-check'", "powershell")]);
    let id = run(&mut f, input).await;
    assert_eq!(f.output(id)["files"][0]["path"], "AGENTS.md");
    let written = std::fs::read_to_string(f.root.join("AGENTS.md")).expect("written");
    assert!(written.contains("Source: verified by running it during initialization"));
    assert!(!written.contains("tcl_"), "a call identity never reaches the file");
}

#[tokio::test]
async fn another_sessions_command_is_not_evidence_for_this_task() {
    let mut other = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    other.service().commands_required().wait_until_ready().await.expect("runtime verified");
    let elsewhere = ran(&mut other, "shell.powershell", "Write-Output 'aura-init-other'").await;
    let mut f = other.additional_session(PermissionMode::WorkspaceReadWrite, TOOLS);
    mark_init(&f);
    let mut input = submission();
    input["commands"]["entries"] = json!([{"command":"Write-Output 'aura-init-other'",
        "purpose":"Print a marker","shell":"powershell",
        "evidence":{"kind":"execution_verified","tool_call_id":elsewhere}}]);
    let call = run(&mut f, input).await;
    let message = refusal(&f, call);
    assert!(message.contains("is not a shell.powershell or shell.cmd call of this /init task"));
}

#[tokio::test]
async fn secrets_local_paths_and_stale_targets_are_refused_before_anyone_is_asked() {
    let f = init_fixture(PermissionMode::ReadOnly).await;
    std::fs::write(f.root.join("AGENTS.md"), "# Mine\n").expect("a file the model did not read");
    let mut input = submission();
    input["coding_style"] =
        format!("Use the token ghp_{} for releases.", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8")
            .into();
    input["testing"] = "Fixtures live in C:\\Users\\someone\\fixtures.".into();
    let batch = f.admit(&[("workspace_init.complete", input)]);
    let id = batch.batch.calls[0].tool_call_id;
    assert!(
        batch.calls.is_empty(),
        "a refused submission is never planned, so no approval can be requested"
    );
    f.execute(batch).await;
    let message = refusal(&f, id);
    assert!(message.contains("secret"), "{message}");
    assert!(message.contains("local_path"), "{message}");
    assert!(message.contains("AGENTS.md already exists; read it and pass its logical_hash"));
    assert_eq!(std::fs::read_to_string(f.root.join("AGENTS.md")).expect("kept"), "# Mine\n");
}

#[tokio::test]
async fn three_refusals_end_the_attempts_and_ordinary_turns_cannot_write() {
    let mut f = init_fixture(PermissionMode::FullAccess).await;
    let mut broken = submission();
    broken["testing"] = "".into();
    for attempt in 1..=3 {
        let call = run(&mut f, broken.clone()).await;
        let message = refusal(&f, call);
        assert!(message.contains(&format!("(attempt {attempt} of 3)")), "{message}");
    }
    let call = run(&mut f, submission()).await;
    let message = refusal(&f, call);
    assert!(message.starts_with("workspace_init.attempts_exhausted"), "{message}");
    assert!(!f.root.join("AGENTS.md").exists());

    let mut ordinary = f.additional_session(PermissionMode::FullAccess, TOOLS);
    let call = run(&mut ordinary, submission()).await;
    let message = refusal(&ordinary, call);
    assert!(message.starts_with("workspace_init.not_an_init_task"), "{message}");
}
