//! B4 acceptance uses real managed processes and isolated stores.
use super::*;
use aura_windows_platform::SecretBytes;
use serde_json::{Value, json};
const TOOLS: &[&str] = &[
    "terminal.open",
    "terminal.write",
    "terminal.read",
    "terminal.request_sensitive_input",
    "terminal.close",
    "shell.cmd",
];
async fn run(f: &mut Fixture, name: &str, input: Value) -> Value {
    let batch = f.admit(&[(name, input)]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let group = f.execute(batch).await;
    let output = f.output(call);
    f.continue_after_tools(batch_id, &group);
    output
}
async fn pending(f: &Fixture) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let value = crate::r03_tests::success(
                &f.runtime,
                "terminal.user.sensitive.pending",
                json!({"session_id":f.turn.session_id}),
            )
            .await;
            if !value["requests"].as_array().expect("pending requests").is_empty() {
                return value["requests"][0].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("request visible without secret data")
}
#[tokio::test]
async fn l02_b4_sensitive_cancellation_returns_status_only_and_clears_pending() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":opened["terminal_id"],"prompt":"测试输入"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
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
    assert_eq!(
        crate::r03_tests::success(
            &f.runtime,
            "terminal.user.sensitive.pending",
            json!({"session_id":f.turn.session_id})
        )
        .await["requests"],
        json!([])
    );
    run(&mut f, "terminal.close", json!({"terminal_id":opened["terminal_id"]})).await;
}
#[tokio::test]
async fn l02_b4_sensitive_echo_is_not_in_the_model_record_or_later_output_and_cannot_be_replayed() {
    use aura_windows_user_contract::TerminalSensitiveReplyV1;
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let id = opened["terminal_id"].clone();
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"set /p L02_VALUE=secret: \n","wait_ms":300}),
    )
    .await;
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":id,"prompt":"临时测试输入"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
        let reply: TerminalSensitiveReplyV1 = serde_json::from_value(
            json!({"session_id":f.turn.session_id,"tool_call_id":request["tool_call_id"]}),
        )
        .expect("L02 test fact");
        let result = f
            .service()
            .commands_required()
            .terminal_sensitive_submit(
                reply.clone(),
                SecretBytes::new(b"temp-b4-echo-value".to_vec()),
            )
            .await
            .expect("L02 test fact");
        assert_eq!(
            serde_json::to_value(result).expect("L02 test fact"),
            json!({"status":"completed"})
        );
        assert!(
            f.service()
                .commands_required()
                .terminal_sensitive_submit(reply, SecretBytes::new(b"second-test-value".to_vec()))
                .await
                .is_err()
        );
    };
    let (group, ()) = tokio::join!(f.execute(batch), answer);
    assert_eq!(f.output(call), json!({"status":"completed"}));
    f.continue_after_tools(batch_id, &group);
    run(
        &mut f,
        "terminal.write",
        json!({"terminal_id":id,"input":"echo %L02_VALUE%\necho L02-AFTER\n","wait_ms":1000}),
    )
    .await;
    let output = run(&mut f, "terminal.read", json!({"terminal_id":id,"max_bytes":65536})).await;
    assert!(!output.to_string().contains("temp-b4-echo-value"), "{output}");
    assert!(output.to_string().contains("安全输入回显已屏蔽"), "{output}");
    assert!(output.to_string().contains("L02-AFTER"));
    {
        let store = f.service().broker.lock().expect("L02 test fact");
        let mut query = store
            .database
            .connection()
            .prepare("SELECT payload FROM events")
            .expect("L02 test fact");
        for payload in query.query_map([], |row| row.get::<_, String>(0)).expect("L02 test fact") {
            assert!(!payload.expect("L02 test fact").contains("temp-b4-echo-value"));
        }
        drop(query);
        drop(store);
    }
    run(&mut f, "terminal.close", json!({"terminal_id":id})).await;
}
#[tokio::test]
async fn l02_b4_sensitive_tool_deadline_cancels_without_sending_any_input() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    f.settings = ToolSettingsSnapshotV1::new(
        f.settings.revision,
        f.settings.max_dynamic_tool_schemas,
        f.settings.max_concurrent_tool_calls,
        1000,
        f.settings.max_retries,
    )
    .expect("L02 test fact");
    let output = run(
        &mut f,
        "terminal.request_sensitive_input",
        json!({"terminal_id":opened["terminal_id"],"prompt":"不填写，测试工具时限"}),
    )
    .await;
    assert_eq!(output, json!({"status":"cancelled"}));
    assert_eq!(
        crate::r03_tests::success(
            &f.runtime,
            "terminal.user.sensitive.pending",
            json!({"session_id":f.turn.session_id})
        )
        .await["requests"],
        json!([])
    );
    run(&mut f, "terminal.close", json!({"terminal_id":opened["terminal_id"]})).await;
}
#[tokio::test]
async fn l02_b4_sensitive_wrong_session_is_rejected_and_takeover_cancels_the_live_request() {
    let mut f = Fixture::with_exposure(PermissionMode::WorkspaceReadWrite, TOOLS).await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd"})).await;
    let batch = f.admit(&[(
        "terminal.request_sensitive_input",
        json!({"terminal_id":opened["terminal_id"],"prompt":"等待接管"}),
    )]);
    let call = batch.batch.calls[0].tool_call_id;
    let batch_id = batch.batch.tool_batch_id;
    let answer = async {
        let request = pending(&f).await;
        let reply = aura_windows_user_contract::TerminalSensitiveReplyV1 {
            session_id: SessionId::new(),
            tool_call_id: serde_json::from_value(request["tool_call_id"].clone())
                .expect("L02 test fact"),
        };
        assert!(
            f.service()
                .commands_required()
                .terminal_sensitive_submit(reply, SecretBytes::new(b"wrong-session-value".to_vec()))
                .await
                .is_err()
        );
        crate::r03_tests::success(&f.runtime,"terminal.user.control",json!({"session_id":f.turn.session_id,"terminal_id":opened["terminal_id"],"controller":"user"})).await;
    };
    let (group, ()) = tokio::join!(f.execute(batch), answer);
    assert_eq!(f.output(call), json!({"status":"cancelled"}));
    f.continue_after_tools(batch_id, &group);
    run(&mut f, "terminal.close", json!({"terminal_id":opened["terminal_id"]})).await;
}
#[tokio::test]
async fn l02_b4_process_page_lists_only_running_agent_work_and_stops_the_owned_job() {
    let mut f = Fixture::with_exposure(
        PermissionMode::WorkspaceReadWrite,
        &["terminal.open", "terminal.close", "shell.cmd"],
    )
    .await;
    let user = crate::r03_tests::success(
        &f.runtime,
        "terminal.user.open",
        json!({"session_id":f.turn.session_id,"shell":"cmd","columns":80,"rows":24}),
    )
    .await;
    let opened = run(&mut f, "terminal.open", json!({"shell":"cmd","title":"后台终端"})).await;
    let background = run(&mut f,"shell.cmd",json!({"script":"start \"\" /b cmd /d /c \"ping -n 120 127.0.0.1 >nul\"\necho foreground","timeout_ms":10000,"max_output_bytes":4096})).await;
    let list = crate::r03_tests::success(
        &f.runtime,
        "agent.process.list",
        json!({"session_id":f.turn.session_id}),
    )
    .await;
    assert_eq!(list["processes"].as_array().expect("running processes").len(), 2, "{list}");
    assert!(!list.to_string().contains(user["terminal_id"].as_str().expect("user id")));
    assert!(
        aura_persistence_v2::execution::process::running_shells(
            f.service().broker.lock().expect("journal").database.connection(),
            f.turn.session_id,
            &[]
        )
        .expect("no current runtime jobs")
        .is_empty(),
        "a previous runtime's durable Running record does not prove a live process"
    );
    for process in list["processes"].as_array().expect("L02 test fact") {
        if process["kind"] == "terminal" {
            assert!(
                process["command"].as_str().expect("L02 test fact").contains("cmd.exe"),
                "terminal must show the running command, not just its title: {process}"
            );
        }
        crate::r03_tests::success(
            &f.runtime,
            "agent.process.stop",
            json!({"session_id":f.turn.session_id,"tool_call_id":process["tool_call_id"]}),
        )
        .await;
    }
    let _ = background;
    assert_eq!(
        crate::r03_tests::success(
            &f.runtime,
            "agent.process.list",
            json!({"session_id":f.turn.session_id})
        )
        .await["processes"],
        json!([])
    );
    crate::r03_tests::success(
        &f.runtime,
        "terminal.user.close",
        json!({"session_id":f.turn.session_id,"terminal_id":user["terminal_id"]}),
    )
    .await;
    assert!(!opened["terminal_id"].is_null());
}
