//! Goes through the real builtin release, broker and ConPTY.
use super::*;
use serde_json::{Value, json};

#[test]
fn four_terminal_tools_are_published_without_user_terminal_access() {
    let release = aura_tool_registry::agent_release::release().expect("builtin release");
    for name in ["terminal.open", "terminal.write", "terminal.read", "terminal.close"] {
        assert!(release.entry(name).is_some(), "missing {name}");
    }
    assert!(release.entry("terminal.user.open").is_none());
}

async fn run(f: &mut Fixture, name: &str, arguments: Value) -> (ToolCallId, Value) {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let output = f.output(id);
    f.continue_after_tools(batch_id, &group);
    (id, output)
}
async fn rejected(f: &mut Fixture, name: &str, arguments: Value) -> String {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let result = {
        let journal = f.service().broker.lock().expect("journal");
        let result = tool::execution::call(journal.database.connection(), id)
            .expect("paired result")
            .result
            .expect("result");
        assert!(!matches!(result.outcome, ToolResultOutcomeV1::Succeeded));
        serde_json::to_string(&result).expect("failure evidence")
    };
    f.continue_after_tools(batch_id, &group);
    result
}
const TOOLS: &[&str] =
    &["terminal.open", "terminal.write", "terminal.read", "terminal.close", "shell.cmd"];
#[tokio::test]
async fn real_agent_cmd_persists_plain_text_and_takeover_pauses_read_and_write() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let (_, opened) =
        run(&mut f, "terminal.open", json!({"shell":"cmd","title":"Agent CMD"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    let native = f
        .service()
        .commands_required()
        .native_terminal_for_test(f.turn.session_id, &id)
        .expect("owned ConPTY");
    let weak = Arc::downgrade(&native);
    drop(native);
    let (_,output)=run(&mut f,"terminal.write",json!({"terminal_id":id,"input":"echo L02-AGENT-READY\n","wait_ms":2000,"until_text":"L02-AGENT-READY"})).await;
    assert!(output["output"].as_str().expect("text").contains("L02-AGENT-READY"));
    assert!(!output.to_string().contains("\\u001b"));
    let listed = crate::r03_tests::success(
        &f.runtime,
        "terminal.user.list",
        json!({"session_id":f.turn.session_id}),
    )
    .await;
    assert_eq!(listed["terminals"][0]["owner"], "agent");
    // Before a takeover the desktop may answer a cursor report, but it cannot type commands.
    let typed = crate::r03_tests::request(
        &f.runtime,
        "terminal.user.write",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"input":"echo USER-TYPED\r","interrupt":false}),
    )
    .await;
    assert!(typed["error"]["message"].as_str().is_some_and(|m| m.contains("接管")), "{typed}");
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.write",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"input":"\u{1b}[5;10R","interrupt":false}),
    )
    .await;
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.control",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"controller":"user"}),
    )
    .await;
    for name in ["terminal.write", "terminal.read"] {
        let error = rejected(
            &mut f,
            name,
            if name.ends_with("write") {
                json!({"terminal_id":id,"input":"echo forbidden>forbidden.txt\n","wait_ms":0})
            } else {
                json!({"terminal_id":id})
            },
        )
        .await;
        assert!(error.contains("用户已接管") && error.contains("交还"), "{error}");
    }
    assert!(!f.root.join("forbidden.txt").exists());
    crate::r03_tests::success(&f.runtime,"terminal.user.write",json!({"session_id":f.turn.session_id,"terminal_id":id,"input":"echo UI-OWNER\r","interrupt":false})).await;
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.control",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"controller":"agent"}),
    )
    .await;
    let (_, output) = run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo L02-HAND-BACK\n","until_text":"L02-HAND-BACK"}),
    )
    .await;
    assert!(output["output"].as_str().expect("text").contains("L02-HAND-BACK"));
    let (_, read) = run(&mut f, "terminal.read", json!({"terminal_id":id,"max_bytes":65536})).await;
    assert!(read["output"].as_str().expect("plain durable output").contains("L02-AGENT-READY"));
    let (_, closed) = run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
    assert_eq!(closed["running"], false);
    assert_eq!(closed["end"]["stop_confirmed"], true);
    assert!(weak.upgrade().is_none(), "kernel ownership released after persistence");
    assert!(
        rejected(&mut f, "terminal.write", json!({"terminal_id":id,"input":"echo replay\n"}))
            .await
            .contains("已结束")
    );
    let (_, after) = run(&mut f, "terminal.read", json!({"terminal_id":id})).await;
    assert!(
        after["output"]
            .as_str()
            .expect("record survives kernel release")
            .contains("L02-AGENT-READY")
    );
    let user = crate::r03_tests::success(
        &f.runtime,
        "terminal.user.open",
        json!({"session_id":f.turn.session_id,"shell":"cmd","columns":80,"rows":24}),
    )
    .await;
    assert!(
        rejected(&mut f, "terminal.read", json!({"terminal_id":user["terminal_id"]}))
            .await
            .contains("not_found_in_session")
    );
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.close",
        json!({"session_id":f.turn.session_id,"terminal_id":user["terminal_id"]}),
    )
    .await;
}
/// A takeover and an output capture both read and save the terminal's record. A takeover
/// that arrives while a capture holds the live terminal must read the record only after the
/// capture saved it; before, it read first and wrote back a stale copy — and the reverse order
/// let a capture undo the takeover, so the model could read while the user had the terminal.
#[tokio::test]
async fn a_takeover_during_an_output_capture_keeps_both_changes() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let (_, opened) = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo L10-6-READY\n","until_text":"L10-6-READY"}),
    )
    .await;
    let session = f.turn.session_id;
    let commands = f.service().commands_required();
    let before =
        commands.agent_terminal_record_for_test(session, &id).expect("record").view.revision;
    let capture = commands
        .agent_terminal_capture_in_progress_for_test(session, &id, 5)
        .expect("the capture holds the live terminal");
    let runtime = f.runtime.clone();
    let terminal = id.clone();
    let takeover = tokio::spawn(async move {
        crate::r03_tests::success(
            &runtime,
            "terminal.user.control",
            json!({"session_id":session,"terminal_id":terminal,"controller":"user"}),
        )
        .await
    });
    // Time for the takeover to reach the record; it once read it here, ahead of the lock.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    tokio::task::spawn_blocking(move || capture.finish())
        .await
        .expect("capture thread")
        .expect("the capture saved");
    takeover.await.expect("takeover");
    let record = commands.agent_terminal_record_for_test(session, &id).expect("record");
    assert_eq!(record.view.controller, aura_windows_user_contract::TerminalControllerV1::User);
    assert_eq!(
        record.view.revision.value(),
        before.value() + 5 + 1,
        "the takeover saved a copy of the record read before the capture saved it"
    );
    let error = rejected(&mut f, "terminal.read", json!({"terminal_id":id})).await;
    assert!(error.contains("用户已接管"), "{error}");
}
#[tokio::test]
async fn readonly_terminal_asks_before_attempt_and_shell_session_approval_covers_open_and_write() {
    let mut f = Fixture::with_exposure(PermissionMode::ReadOnly, TOOLS).await;
    let batch = f.admit(&[("terminal.open", json!({"shell":"cmd"}))]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let call = batch.calls[&id].clone();
    assert!(matches!(
        f.execute_rejecting(batch).await,
        Some(BatchOutcome::AwaitingApproval { .. })
    ));
    assert!(
        tool::execution::call(
            f.service().broker.lock().expect("journal").database.connection(),
            id
        )
        .expect("pending")
        .attempt_id
        .is_none()
    );
    crate::agent_execution::tests::s08::support::approve(&f, &call).await;
    let batch = f
        .service()
        .agent_execution_required()
        .resume_batch(batch_id)
        .expect("same frozen pending batch");
    let group = f.execute(batch).await;
    let opened = f.output(id);
    f.continue_after_tools(batch_id, &group);
    let terminal = opened["terminal_id"].as_str().expect("terminal");
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":terminal,"input":"echo approved\n","until_text":"approved"}),
    )
    .await;
    run(&mut f, "shell.cmd", json!({"script":"echo shared>shared.txt"})).await;
    assert!(f.root.join("shared.txt").exists());
    run(&mut f, "terminal.close", json!({"terminal_id":terminal})).await;
    let mut shell_first = Fixture::with_exposure(PermissionMode::ReadOnly, TOOLS).await;
    let batch = shell_first.admit(&[("shell.cmd", json!({"script":"echo Shell-session"}))]);
    let call = batch.batch.calls[0].tool_call_id;
    crate::agent_execution::tests::s08::support::approve(&shell_first, &batch.calls[&call]).await;
    let batch_id = batch.batch.tool_batch_id;
    let group = shell_first.execute(batch).await;
    shell_first.continue_after_tools(batch_id, &group);
    let (_, open) = run(&mut shell_first, "terminal.open", json!({"shell":"cmd"})).await;
    run(&mut shell_first, "terminal.close", json!({"terminal_id":open["terminal_id"]})).await;
}
