//! A user's own terminal handed to the model with @: the model's terminal tools act on it while the
//! model controls it, reading only what it printed from the handover on; the model cannot close
//! it; the user takes it back with the existing takeover and types again; it stays the user's.
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

async fn desktop(f: &Fixture, method: &str, params: Value) -> Value {
    crate::r03_tests::success(&f.runtime, method, params).await
}

async fn control(f: &Fixture, id: &str, controller: &str) {
    desktop(
        f,
        "terminal.user.control",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"controller":controller}),
    )
    .await;
}

async fn user_types(f: &Fixture, id: &str, input: &str) {
    desktop(
        f,
        "terminal.user.write",
        json!({"session_id":f.turn.session_id,"terminal_id":id,"input":input,"interrupt":false}),
    )
    .await;
}

/// Waits until the terminal's raw output from `from` holds `text`; the offset after it.
async fn printed(f: &Fixture, id: &str, from: u64, text: &str) -> u64 {
    let terminal =
        f.service().commands_required().compat.terminals.get(f.turn.session_id, id).expect("live");
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let page = terminal.read(from, 1 << 20).expect("output");
        if String::from_utf8_lossy(&page.bytes).contains(text) {
            return page.next_offset;
        }
        assert!(Instant::now() < deadline, "{text} never printed");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn running(f: &Fixture, id: &str) -> bool {
    f.service()
        .commands_required()
        .compat
        .terminals
        .get(f.turn.session_id, id)
        .is_ok_and(|terminal| terminal.end().expect("end").is_none())
}

#[tokio::test]
async fn a_terminal_the_user_handed_over_is_the_models_to_use_but_never_to_close() {
    let _limit = crate::test_watchdog::limit(
        "a_terminal_the_user_handed_over_is_the_models_to_use_but_never_to_close",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let session = f.turn.session_id;
    let opened = desktop(
        &f,
        "terminal.user.open",
        json!({"session_id":session,"shell":"cmd","columns":80,"rows":24}),
    )
    .await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    user_types(&f, &id, "echo L21-BEFORE\r").await;
    let after = printed(&f, &id, 0, "L21-BEFORE").await;
    // Off the screen, so no redraw of the screen brings it back after the handover.
    user_types(&f, &id, "cls\r").await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    printed(&f, &id, after, "cls").await;

    // Not handed over yet: the model's tools do not see it.
    let unseen = refused(&mut f, "terminal.read", json!({"terminal_id":id})).await;
    assert!(unseen.contains("not_found_in_session"), "{unseen}");

    let handed =
        desktop(&f, "terminal.user.hand_over", json!({"session_id":session,"terminal_id":id}))
            .await;
    assert_eq!(handed["owner"], "user", "{handed}");
    assert_eq!(handed["controller"], "agent", "{handed}");
    assert_eq!(handed["handed_over"], true, "{handed}");

    let wrote = run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo L21-AFTER\n","wait_ms":10000,"until_text":"L21-AFTER"}),
    )
    .await;
    assert!(wrote["output"].as_str().expect("text").contains("L21-AFTER"), "{wrote}");
    let read =
        run(&mut f, "terminal.read", json!({"terminal_id":id,"line":1,"max_lines":2000})).await;
    let text = read["lines"].as_str().expect("lines");
    // What it printed before the handover reached the model only as the message's attachment.
    assert!(text.contains("L21-AFTER") && !text.contains("L21-BEFORE"), "{text}");

    // It stays the user's: the model cannot close it.
    let close = refused(&mut f, "terminal.close", json!({"terminal_id":id})).await;
    assert!(close.contains("terminal.user_owned"), "{close}");
    assert!(running(&f, &id), "the refused close left it running");

    // The desktop shows it once, still the user's, and the process list is the model's own.
    let list = desktop(&f, "terminal.user.list", json!({"session_id":session})).await;
    let shown = list["terminals"]
        .as_array()
        .expect("terminals")
        .iter()
        .filter(|view| view["terminal_id"] == id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(shown.len(), 1, "{list}");
    assert_eq!(shown[0]["owner"], "user");
    assert_eq!(shown[0]["handed_over"], true);
    let processes = desktop(&f, "agent.process.list", json!({"session_id":session})).await;
    assert!(
        processes["processes"]
            .as_array()
            .expect("processes")
            .iter()
            .all(|process| process["terminal_id"] != id.as_str()),
        "{processes}"
    );
    // Tightened authority stops the model's own processes only.
    f.service().agent().expect("agent").file_authority_withdrawn(session).await.expect("stops");
    assert!(running(&f, &id), "the user's terminal is not the model's process");

    // The user takes it back: the model is paused, the user types.
    control(&f, &id, "user").await;
    let held = refused(&mut f, "terminal.read", json!({"terminal_id":id})).await;
    assert!(held.contains("用户已接管"), "{held}");
    user_types(&f, &id, "echo L21-USER\r").await;
    // Handed over again, the model reads on, the user's turn included.
    let again =
        desktop(&f, "terminal.user.hand_over", json!({"session_id":session,"terminal_id":id}))
            .await;
    assert_eq!(again["controller"], "agent", "{again}");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let read =
            run(&mut f, "terminal.read", json!({"terminal_id":id,"line":1,"max_lines":2000})).await;
        if read["lines"].as_str().expect("lines").contains("L21-USER") {
            break;
        }
        assert!(Instant::now() < deadline, "{read}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // The user closes its tab: it ends, as the user's terminal does.
    let closed =
        desktop(&f, "terminal.user.close", json!({"session_id":session,"terminal_id":id})).await;
    assert_eq!(closed["end"]["reason"], "user_closed", "{closed}");
    assert!(!running(&f, &id));
}

#[tokio::test]
async fn handing_over_the_models_own_terminal_gives_it_back_to_the_model() {
    let _limit = crate::test_watchdog::limit(
        "handing_over_the_models_own_terminal_gives_it_back_to_the_model",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd","title":"Agent CMD"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    control(&f, &id, "user").await;
    let handed = desktop(
        &f,
        "terminal.user.hand_over",
        json!({"session_id":f.turn.session_id,"terminal_id":id}),
    )
    .await;
    assert_eq!(handed["owner"], "agent", "{handed}");
    assert_eq!(handed["controller"], "agent", "{handed}");
    assert!(handed.get("handed_over").is_none(), "{handed}");
    let closed = run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
    assert_eq!(closed["running"], false);
}
