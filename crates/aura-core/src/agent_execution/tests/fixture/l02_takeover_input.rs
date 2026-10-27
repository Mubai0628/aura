//! Typing into an Agent terminal the user took over waits for the terminal and nothing else.
//! Each key used to read the terminal's record from the shared journal twice and to wait for the
//! output capture, which held the terminal while it saved every echo. Typing fell further behind
//! the larger the database was.
use super::*;
use serde_json::{Value, json};
use std::time::Duration;

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

/// The shared journal locked on another thread for `HOLD`, as a long output save holds it. It lets go
/// on its own: the request once blocked on the journal inside the runtime's only thread, so
/// nothing in the test could have released it.
struct JournalHeld {
    until: std::time::Instant,
    thread: std::thread::JoinHandle<()>,
}

const HOLD: Duration = Duration::from_secs(3);

impl JournalHeld {
    fn new(f: &Fixture) -> Self {
        let journal = f.service().broker.clone();
        let (locked, held) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            let _journal = journal.lock().expect("journal");
            locked.send(std::time::Instant::now() + HOLD).ok();
            std::thread::sleep(HOLD);
        });
        let until = held.recv().expect("the journal is held");
        Self { until, thread }
    }

    fn join(self) {
        self.thread.join().expect("journal holder");
    }
}

/// Whether the terminal itself printed `text` before `until`, read from the native output, not the
/// saved record.
async fn printed(
    native: &aura_backend_windows_user::compat::TerminalSession,
    text: &str,
    until: std::time::Instant,
) -> bool {
    while std::time::Instant::now() < until {
        let page = native.read(0, 1 << 20).expect("native output");
        if String::from_utf8_lossy(&page.bytes).contains(text) {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn a_key_typed_into_a_taken_over_terminal_does_not_wait_for_output_persistence() {
    let _limit = crate::test_watchdog::limit(
        "a_key_typed_into_a_taken_over_terminal_does_not_wait_for_output_persistence",
    );
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo L10-7-READY\n","until_text":"L10-7-READY"}),
    )
    .await;
    let session = f.turn.session_id;
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.control",
        json!({"session_id":session,"terminal_id":id,"controller":"user"}),
    )
    .await;
    let native = f
        .service()
        .commands_required()
        .native_terminal_for_test(session, &id)
        .expect("owned ConPTY");

    let held = JournalHeld::new(&f);
    // The result, 77107, is not in the typed text, so seeing it means the shell ran the command.
    let started = std::time::Instant::now();
    let typed = crate::r03_tests::request(
        &f.runtime,
        "terminal.user.write",
        json!({"session_id":session,"terminal_id":id,"input":"set /a 70000+7107\r","interrupt":false}),
    )
    .await;
    let waited = started.elapsed();
    let ran = printed(&native, "77107", held.until).await;
    held.join();
    assert!(
        waited < Duration::from_secs(1),
        "the key waited {waited:?} for the output to be saved"
    );
    assert!(typed.get("error").is_none(), "{typed}");
    assert!(ran, "the typed command did not run while the output could not be saved");

    // Nothing typed meanwhile is lost: once handed back, the model reads it from the saved output.
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.control",
        json!({"session_id":session,"terminal_id":id,"controller":"agent"}),
    )
    .await;
    let read = run(&mut f, "terminal.read", json!({"terminal_id":id,"max_bytes":65536})).await;
    assert!(read["output"].as_str().expect("saved output").contains("77107"), "{read}");
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}
