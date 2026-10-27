//! A model that found its terminal taken over waits for the user to hand it back with an empty
//! `terminal.write`, and hears of the handback at once.
use super::*;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const TOOLS: &[&str] = &["terminal.open", "terminal.write", "terminal.read", "terminal.close"];

async fn run(f: &mut Fixture, name: &str, arguments: Value) -> Value {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let output = f.output(id);
    f.continue_after_tools(batch_id, &group);
    output
}

async fn refused(f: &mut Fixture, name: &str, arguments: Value) -> String {
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

async fn control(f: &Fixture, id: &str, controller: &str) {
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.control",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"controller":controller}),
    )
    .await;
}

#[tokio::test]
async fn an_empty_write_waits_for_the_user_to_hand_the_terminal_back() {
    let _limit =
        crate::test_watchdog::limit("an_empty_write_waits_for_the_user_to_hand_the_terminal_back");
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd","title":"Agent CMD"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    control(&f, &id, "user").await;
    // Refused while the user has it, the model is told how to wait for it.
    let read = refused(&mut f, "terminal.read", json!({"terminal_id":id})).await;
    assert!(read.contains("用户已接管") && read.contains("input 为空"), "{read}");
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.write",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"input":"echo L135-USER-TYPED\r","interrupt":false}),
    )
    .await;

    let batch =
        f.admit(&[("terminal.write", json!({"terminal_id":id,"input":"","wait_ms":20000}))]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let started = Instant::now();
    let (group, ()) = tokio::join!(f.execute(batch), async {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        control(&f, &id, "agent").await;
    });
    let waited = started.elapsed();
    let output = f.output(call);
    f.continue_after_tools(batch_id, &group);
    // It returned with the handback, not at the end of its wait, showing what the user did.
    assert!(
        waited >= Duration::from_millis(1500) && waited < Duration::from_secs(10),
        "{waited:?}"
    );
    assert!(output["output"].as_str().expect("text").contains("L135-USER-TYPED"), "{output}");
    assert_eq!(output["running"], true);

    // Still held when its wait ends, it says so and writes nothing.
    control(&f, &id, "user").await;
    let held =
        refused(&mut f, "terminal.write", json!({"terminal_id":id,"input":"","wait_ms":300})).await;
    assert!(held.contains("仍在操作"), "{held}");
    control(&f, &id, "agent").await;
    let closed = run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
    assert_eq!(closed["running"], false);
}
