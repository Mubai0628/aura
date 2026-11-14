//! A bounded model response must not stop an until-text search at its first page.
use super::*;
use serde_json::{Value, json};

async fn run(f: &mut Fixture, name: &str, arguments: Value) -> Value {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let output = f.output(id);
    f.continue_after_tools(batch_id, &group);
    output
}

#[tokio::test]
async fn l03_b0_until_text_searches_all_new_pages_but_keeps_response_bounded() {
    let _limit = crate::test_watchdog::limit(
        "l03_b0_until_text_searches_all_new_pages_but_keeps_response_bounded",
    );
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["terminal.open", "terminal.write", "terminal.read", "terminal.close"],
        |root| {
            std::fs::write(root.join("pages.cmd"),
                "@echo off\r\nfor /L %%i in (1,1,1200) do echo 0123456789012345678901234567890123456789012345678901234567890123456789\r\nping -n 2 127.0.0.1>nul\r\necho ready>wait-finished.txt\r\necho B0-PAGES-FINISHED\r\n").expect("local fixture");
        },
    ).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    let output = run(
        &mut f,
        "terminal.write",
        json!({
            "terminal_id":id,"input":"call pages.cmd\n","wait_ms":5000,
            "until_text":"B0-PAGES-FINISHED",
        }),
    )
    .await;
    assert!(
        f.root.join("wait-finished.txt").exists(),
        "until_text after 8 KiB must be observed before returning: {output}"
    );
    assert!(output["output"].as_str().expect("bounded output").len() <= 8192);
    assert_eq!(output["truncated"], true, "remaining output stays explicitly pageable");
    // The newest lines show how it finished; all of them page by line number, each line once.
    let newest = run(&mut f, "terminal.read", json!({"terminal_id":id})).await;
    assert!(newest["lines"].as_str().expect("lines").contains("B0-PAGES-FINISHED"), "{newest}");
    let digits = "0123456789".repeat(7);
    let (mut line, mut printed) = (1, 0);
    loop {
        let page =
            run(&mut f, "terminal.read", json!({"terminal_id":id,"line":line,"max_lines":2000}))
                .await;
        assert_eq!(page["first_line"], line, "{page}");
        let lines = page["lines"].as_str().expect("lines");
        printed += lines.lines().filter(|text| *text == digits).count();
        match page["next_line"].as_u64() {
            Some(next) => line = next,
            None => break,
        }
    }
    assert_eq!(printed, 1200, "every printed line once, as the screen showed it");
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}

/// A write with no wait of its own returns the output of a command that keeps printing for longer
/// than a moment, once it has stopped; it does not wait for the whole limit when output stops.
#[tokio::test]
async fn a_write_returns_once_the_output_settles() {
    let _limit = crate::test_watchdog::limit("a_write_returns_once_the_output_settles");
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["terminal.open", "terminal.write", "terminal.read", "terminal.close"],
        |root| {
            std::fs::write(
                root.join("steady.cmd"),
                "@echo off\r\nfor /L %%i in (1,1,5) do (echo STEADY-%%i & ping -n 2 127.0.0.1>nul)\r\necho STEADY-DONE\r\n",
            )
            .expect("local fixture");
            std::fs::write(
                root.join("pause.cmd"),
                "@echo off\r\necho PAUSE-START\r\nping -n 9 127.0.0.1>nul\r\necho PAUSE-LATE\r\n",
            )
            .expect("local fixture");
        },
    )
    .await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    let steady =
        run(&mut f, "terminal.write", json!({"terminal_id":id,"input":"call steady.cmd\n"})).await;
    assert!(
        steady["output"].as_str().expect("plain output").contains("STEADY-DONE"),
        "printing about once a second for five seconds is one answer: {steady}"
    );
    let started = std::time::Instant::now();
    let paused =
        run(&mut f, "terminal.write", json!({"terminal_id":id,"input":"call pause.cmd\n"})).await;
    let text = paused["output"].as_str().expect("plain output");
    assert!(text.contains("PAUSE-START") && !text.contains("PAUSE-LATE"), "{paused}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(6),
        "a quiet terminal answers without waiting out the limit: {:?}",
        started.elapsed()
    );
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}

/// A command line that names the text it waits for, as `echo DONE` waiting for DONE does, is
/// answered by the command's output, not by the echo of the line typed.
#[tokio::test]
async fn an_until_text_in_the_typed_line_is_looked_for_after_its_echo() {
    let _limit =
        crate::test_watchdog::limit("an_until_text_in_the_typed_line_is_looked_for_after_its_echo");
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["terminal.open", "terminal.write", "terminal.read", "terminal.close"],
        |root| {
            std::fs::write(
                root.join("mark.cmd"),
                "@echo off\r\nping -n 3 127.0.0.1>nul\r\necho %1\r\n",
            )
            .expect("local fixture");
        },
    )
    .await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    let marked = run(
        &mut f,
        "terminal.write",
        json!({
            "terminal_id":id,"input":"call mark.cmd MARK-DONE\n","until_text":"MARK-DONE",
            "wait_ms":10000,
        }),
    )
    .await;
    let text = marked["output"].as_str().expect("plain output");
    let (_, printed) = text.split_once('\n').unwrap_or_default();
    assert!(
        printed.contains("MARK-DONE"),
        "the echo names it; the script prints it about 2 s later: {marked}"
    );
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}

/// A command that is silent for a while before it prints, as on a busy machine or a slow remote
/// shell, is still one answer: the write returns once the shell's prompt is back.
#[tokio::test]
async fn a_write_waits_through_a_slow_start_and_returns_at_the_prompt() {
    let _limit =
        crate::test_watchdog::limit("a_write_waits_through_a_slow_start_and_returns_at_the_prompt");
    // Inside a script, so the shell shows nothing while it waits; a command typed at the prompt
    // retitles the console, which redraws it.
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["terminal.open", "terminal.write", "terminal.read", "terminal.close"],
        |root| {
            std::fs::write(
                root.join("slow.cmd"),
                "@echo off\r\nping -n 3 127.0.0.1>nul\r\necho SLOW-START-DONE\r\n",
            )
            .expect("local fixture");
        },
    )
    .await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    let started = std::time::Instant::now();
    let slow =
        run(&mut f, "terminal.write", json!({"terminal_id":id,"input":"call slow.cmd\n"})).await;
    let text = slow["output"].as_str().expect("plain output");
    assert!(text.contains("SLOW-START-DONE"), "about 2 s of silence before printing: {slow}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "the prompt ends the wait, not the limit: {:?}",
        started.elapsed()
    );
    // The quiet alone answers no sooner than 3 s after the last output, so an answer within that
    // comes from the prompt.
    let started = std::time::Instant::now();
    let quick =
        run(&mut f, "terminal.write", json!({"terminal_id":id,"input":"echo QUICK-DONE\n"})).await;
    assert!(quick["output"].as_str().expect("plain output").contains("QUICK-DONE"), "{quick}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(3),
        "a command that is done answers at its prompt: {:?}",
        started.elapsed()
    );
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}

/// A line a command redraws in place reads in the write's answer as the screen shows it, not as
/// every frame run together; closing answers with what the screen showed last.
#[tokio::test]
async fn a_write_answers_with_the_lines_the_screen_shows() {
    let _limit = crate::test_watchdog::limit("a_write_answers_with_the_lines_the_screen_shows");
    let mut f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["terminal.open", "terminal.write", "terminal.read", "terminal.close"],
        |root| {
            // Each step goes back to the start of the line and prints over the one before.
            std::fs::write(
                root.join("redraw.ps1"),
                "[Console]::Write('STEP-1'); Start-Sleep -Milliseconds 400\r\n[Console]::Write(\"`rSTEP-2\"); Start-Sleep -Milliseconds 400\r\n[Console]::Write(\"`rSTEP-3\"); [Console]::WriteLine()\r\n",
            )
            .expect("local fixture");
        },
    )
    .await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].as_str().expect("terminal id").to_owned();
    let written = run(
        &mut f,
        "terminal.write",
        json!({
            "terminal_id":id,
            "input":"powershell -NoProfile -ExecutionPolicy Bypass -File redraw.ps1\n",
            "wait_ms":15000,
        }),
    )
    .await;
    let text = written["output"].as_str().expect("lines");
    assert!(text.lines().any(|line| line == "STEP-3"), "{written}");
    assert!(!text.contains("STEP-1STEP-2"), "{written}");
    let closed = run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
    let text = closed["output"].as_str().expect("lines");
    assert!(text.lines().any(|line| line == "STEP-3"), "{closed}");
    assert!(!text.contains("STEP-1STEP-2"), "{closed}");
}
