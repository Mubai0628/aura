//! A secure input waits for the user the way an approval does, and reaches the terminal only
//! while the terminal still shows the prompt it was asked at. Real managed cmd terminals.
use super::l02_sensitive::{TOOLS, pending, run};
use super::*;
use aura_windows_platform::SecretBytes;
use aura_windows_user_contract::TerminalSensitiveReplyV1;
use serde_json::{Value, json};
use std::time::Duration;

/// The conversation's entry in the activity the sidebar and the notifications read.
async fn activity(f: &Fixture) -> Value {
    let value = crate::r03_tests::success(&f.runtime, "agent.sessions.activity", json!({})).await;
    value["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|entry| entry["session_id"] == json!(f.turn.session_id))
        .cloned()
        .unwrap_or(Value::Null)
}

/// What the terminal has recorded so far, as the model reads it.
fn recorded(f: &Fixture, id: &str) -> String {
    let store = f.service().broker.lock().expect("journal");
    let record =
        aura_persistence_v2::terminal::get(store.database.connection(), f.turn.session_id, id)
            .expect("terminal record");
    aura_persistence_v2::terminal::read(store.database.connection(), &record, 0, 65536)
        .expect("terminal output")
        .output
}

async fn recorded_until(f: &Fixture, id: &str, text: &str) {
    tokio::time::timeout(Duration::from_secs(15), async {
        while !recorded(f, id).contains(text) {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the terminal showed {text}: {}", recorded(f, id)));
}

fn reply(f: &Fixture, request: &Value) -> TerminalSensitiveReplyV1 {
    serde_json::from_value(
        json!({"session_id":f.turn.session_id,"tool_call_id":request["tool_call_id"]}),
    )
    .expect("reply")
}

async fn submit(f: &Fixture, request: &Value, value: &[u8]) -> anyhow::Result<Value> {
    let result = f
        .service()
        .commands_required()
        .terminal_sensitive_submit(reply(f, request), SecretBytes::new(value.to_vec()))
        .await?;
    Ok(serde_json::to_value(result)?)
}

/// What the model is told about a call that failed.
/// What the model was told of a secure input the terminal had moved on from: nothing was typed, so
/// the call is refused over a changed state, not a failed tool.
fn refusal(f: &Fixture, call: ToolCallId) -> String {
    let store = f.service().broker.lock().expect("journal");
    let record = aura_persistence_v2::tool::execution::call(store.database.connection(), call)
        .expect("recorded call");
    match record.result.expect("finished call").outcome {
        ToolResultOutcomeV1::OperationRejected { error } => {
            assert_eq!(error.code.as_str(), "terminal.sensitive.prompt_gone", "{error:?}");
            assert_eq!(error.category, aura_domain_common::ErrorCategory::Conflict, "{error:?}");
            error.message
        }
        other => panic!("expected the call to be refused, got {other:?}"),
    }
}

async fn none_pending(f: &Fixture) {
    let left = crate::r03_tests::success(
        &f.runtime,
        "terminal.user.sensitive.pending",
        json!({"session_id":f.turn.session_id}),
    )
    .await;
    assert_eq!(left["requests"], json!([]));
}

#[tokio::test]
async fn an_open_secure_input_shows_the_conversation_as_waiting_for_the_user() {
    let _limit = crate::test_watchdog::limit(
        "an_open_secure_input_shows_the_conversation_as_waiting_for_the_user",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let before = activity(&f).await;
    assert_eq!(before["state"], "running", "{before}");
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":opened["terminal_id"],"prompt":"测试等待状态"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
        let waiting = activity(&f).await;
        assert_eq!(waiting["state"], "waiting", "{waiting}");
        assert_eq!(waiting["waiting_for"], "sensitive_input", "{waiting}");
        crate::r03_tests::success(
            &f.runtime,
            "terminal.user.sensitive.cancel",
            json!({"session_id":f.turn.session_id,"tool_call_id":request["tool_call_id"]}),
        )
        .await;
    };
    let (group, ()) = tokio::join!(f.execute(batch), answer);
    assert_eq!(f.output(call), json!({"status":"cancelled"}));
    f.continue_after_tools(batch_id, &group);
    let after = activity(&f).await;
    assert_eq!(after["state"], "running", "{after}");
    assert!(after.get("waiting_for").is_none(), "{after}");
    run(&mut f, "terminal.close", json!({"terminal_id":opened["terminal_id"]})).await;
}

#[tokio::test]
async fn a_secure_input_is_not_typed_once_the_prompt_it_was_asked_at_has_gone() {
    let _limit = crate::test_watchdog::limit(
        "a_secure_input_is_not_typed_once_the_prompt_it_was_asked_at_has_gone",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    // A prompt that gives up by itself when nobody answers in time, as a login or sudo does.
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"choice /T 6 /D N /M L02-GONE-PROMPT\n","wait_ms":5000,"until_text":"[Y,N]?"}),
    )
    .await;
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":id,"prompt":"测试提示消失"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
        // The prompt gives up and cmd shows its own prompt again.
        recorded_until(&f, &id, "[Y,N]?N").await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let refused = submit(&f, &request, b"l02-gone-value").await;
        let error = refused.expect_err("nothing is typed once the prompt has gone");
        assert!(error.to_string().contains("terminal.sensitive.prompt_gone"), "{error:#}");
    };
    let (group, ()) = tokio::join!(f.execute(batch), answer);
    let told = refusal(&f, call);
    assert!(told.starts_with("terminal.sensitive.prompt_gone"), "{told}");
    assert!(told.contains("没有输入"), "{told}");
    f.continue_after_tools(batch_id, &group);
    none_pending(&f).await;
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo L02-AFTER-GONE\n","wait_ms":3000,"until_text":"L02-AFTER-GONE"}),
    )
    .await;
    recorded_until(&f, &id, "L02-AFTER-GONE").await;
    let output = recorded(&f, &id);
    // cmd never ran it as a command, and no echo of it had to be hidden.
    assert!(!output.contains("l02-gone-value"), "{output}");
    assert!(!output.contains("安全输入回显已屏蔽"), "{output}");
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}

#[tokio::test]
async fn output_after_the_request_keeps_the_input_back_even_while_the_program_still_waits() {
    let _limit = crate::test_watchdog::limit(
        "output_after_the_request_keeps_the_input_back_even_while_the_program_still_waits",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    // Another program of the same console prints while the prompt still waits. The caret keeps
    // the marker out of the command's own echo, so only the late output shows it.
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"start \"\" /b cmd /d /c \"ping -n 6 127.0.0.1 >nul & echo L02-LATE-^LINE\" & set /p L02_VALUE=L02-LATE-PROMPT: \n","wait_ms":5000,"until_text":"L02-LATE-PROMPT: "}),
    )
    .await;
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":id,"prompt":"测试新的输出"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
        recorded_until(&f, &id, "L02-LATE-LINE").await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        let refused = submit(&f, &request, b"l02-late-value").await;
        let error = refused.expect_err("output after the request keeps the input back");
        assert!(error.to_string().contains("terminal.sensitive.prompt_gone"), "{error:#}");
    };
    let (group, ()) = tokio::join!(f.execute(batch), answer);
    assert!(refusal(&f, call).starts_with("terminal.sensitive.prompt_gone"));
    f.continue_after_tools(batch_id, &group);
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}

#[tokio::test]
async fn a_repaint_after_the_user_resizes_the_terminal_still_lets_the_input_through() {
    let _limit = crate::test_watchdog::limit(
        "a_repaint_after_the_user_resizes_the_terminal_still_lets_the_input_through",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"set /p L02_VALUE=L02-RESIZE-PROMPT: \n","wait_ms":5000,"until_text":"L02-RESIZE-PROMPT: "}),
    )
    .await;
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":id,"prompt":"测试重绘"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
        let before = recorded(&f, &id).len();
        // The user opens the terminal's tab: it is resized, and ConPTY paints its screen again.
        crate::r03_tests::success(
            &f.runtime,
            "terminal.user.resize",
            json!({"session_id":f.turn.session_id,"terminal_id":id,"columns":132,"rows":40}),
        )
        .await;
        let _ = tokio::time::timeout(Duration::from_secs(3), async {
            while recorded(&f, &id).len() == before {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        tokio::time::sleep(Duration::from_millis(800)).await;
        let typed = submit(&f, &request, b"l02-resize-value").await.expect("the prompt is shown");
        assert_eq!(typed, json!({"status":"completed"}));
    };
    let (group, ()) = tokio::join!(f.execute(batch), answer);
    assert_eq!(f.output(call), json!({"status":"completed"}));
    f.continue_after_tools(batch_id, &group);
    // The value reached the prompt: echoing it shows only the mask.
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo [%L02_VALUE%]\n","wait_ms":3000,"until_text":"安全输入回显已屏蔽"}),
    )
    .await;
    let output = recorded(&f, &id);
    assert!(output.contains("[[安全输入回显已屏蔽]]"), "{output}");
    assert!(!output.contains("l02-resize-value"), "{output}");
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}
