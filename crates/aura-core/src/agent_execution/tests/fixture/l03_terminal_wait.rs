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
    let mut offset = output["next_offset"].as_u64().expect("next offset");
    let mut tail = String::new();
    loop {
        let page = run(
            &mut f,
            "terminal.read",
            json!({"terminal_id":id,"offset":offset,"max_bytes":65536}),
        )
        .await;
        offset = page["next_offset"].as_u64().expect("next offset");
        tail.push_str(page["output"].as_str().expect("plain output"));
        if page["truncated"] == false {
            break;
        }
    }
    assert!(tail.contains("B0-PAGES-FINISHED"));
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}
