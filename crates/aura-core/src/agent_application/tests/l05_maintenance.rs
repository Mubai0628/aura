//! Only the configured loopback Provider is scripted; task completion, lease,
//! request compilation, transport, permission barrier and Vault writer are real.
use super::*;
use aura_memory::{MemoryMaintenanceState, MemoryScope, MemoryWorkspace};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Script {
    url: String,
    requests: Arc<Mutex<Vec<Value>>>,
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Script {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Script {
    async fn new(readonly: bool, response: String, park: bool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback");
        let url = format!("http://{}/v1", listener.local_addr().expect("bound"));
        let requests = Arc::new(Mutex::new(vec![]));
        let saved = requests.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let notice = entered.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let unblock = release.clone();
        let task = tokio::spawn(async move {
            for ordinal in 0..3 {
                let (mut socket, _) = listener.accept().await.expect("local request");
                let mut raw = vec![];
                let mut chunk = [0; 8192];
                let body = loop {
                    let n = socket.read(&mut chunk).await.expect("request");
                    assert!(n > 0);
                    raw.extend_from_slice(&chunk[..n]);
                    assert!(raw.len() < 2 * 1024 * 1024);
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                        assert!(
                            !headers.contains("authorization:") && !headers.contains("cookie:")
                        );
                        let length = headers
                            .lines()
                            .find_map(|s| s.strip_prefix("content-length:"))
                            .expect("length")
                            .trim()
                            .parse::<usize>()
                            .expect("length");
                        if raw.len() >= end + 4 + length {
                            break serde_json::from_slice::<Value>(&raw[end + 4..end + 4 + length])
                                .expect("JSON");
                        }
                    }
                };
                saved.lock().expect("requests").push(body.clone());
                let delta = if ordinal == 0 {
                    let (name, args) = if readonly {
                        ("fs.read", json!({"path":"source.txt"}))
                    } else {
                        (
                            "fs.write",
                            json!({"path":"result.txt","text":"completed work","expected":{"expected_kind":"absent"}}),
                        )
                    };
                    let definition = aura_tool_registry::agent_release::release()
                        .expect("release")
                        .entry(name)
                        .expect("tool");
                    let wire = provider_wire_tool_name(name, definition.definition.tool_id);
                    assert!(
                        body["tools"]
                            .as_array()
                            .expect("exposed tools")
                            .iter()
                            .any(|t| t["function"]["name"] == wire),
                        "fixture can only call a loaded wire name"
                    );
                    json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_actual_work","type":"function","function":{"name":wire,"arguments":provider_fixture::flat_call("Do the requested temporary work",args)}}]})
                } else if ordinal == 1 {
                    json!({"role":"assistant","content":if readonly{"已检查本机测试文件。".repeat(250)}else{"The requested work is complete.".into()}})
                } else {
                    assert!(!body["tools"].as_array().is_some_and(|a| !a.is_empty()));
                    assert!(body.to_string().contains("Untrusted completed-task data"));
                    if park {
                        let partial = json!({"id":"fixture-maintenance","object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}});
                        let final_chunk = json!({"id":"fixture-maintenance","object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{"content":response},"finish_reason":"stop"}],"usage":{"prompt_tokens":20,"completion_tokens":20,"total_tokens":40}});
                        let prefix = format!("data: {partial}\n\n");
                        let suffix = format!("data: {final_chunk}\n\ndata: [DONE]\n\n");
                        socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{prefix}",prefix.len()+suffix.len()).as_bytes()).await.expect("partial usage");
                        notice.notify_one();
                        unblock.notified().await;
                        socket.write_all(suffix.as_bytes()).await.expect("released response");
                        continue;
                    }
                    notice.notify_one();
                    json!({"role":"assistant","content":response})
                };
                provider_fixture::send_with(&mut socket, ordinal, delta, None).await;
            }
        });
        Self { url, requests, entered, release, task }
    }
}
async fn exercise(
    readonly: bool,
    response: String,
    park: bool,
) -> (
    crate::r03_tests::Fixture,
    crate::RuntimeState,
    Script,
    Arc<AgentApplication>,
    AgentRunId,
    MemoryWorkspace,
) {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let script = Script::new(readonly, response, park).await;
    let model = profile(&runtime, &script.url).await;
    let root = host.root.path().join("maintenance-workspace");
    std::fs::create_dir(&root).expect("workspace");
    std::fs::write(root.join("source.txt"), "temporary source").expect("source");
    let created = crate::r03_tests::success(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Memory maintenance"}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace id");
    let session = create_session(
        &runtime,
        workspace,
        if readonly { PermissionMode::ReadOnly } else { PermissionMode::WorkspaceReadWrite },
    )
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("services") };
    let binding =
        services.persistence.session_model_binding_state(session).expect("session binding");
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":session,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    let app = services.agent_required();
    let view = app.tool_settings().expect("settings");
    let mut fixed = view.fixed_tool_ids.clone();
    let write = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("fs.write")
        .expect("write")
        .definition
        .tool_id;
    if !fixed.contains(&write) {
        fixed.push(write);
        fixed.sort();
    }
    app.update_tool_settings(
        CommandInvocationId::new(),
        aura_tool_contract::AgentToolSettingsUpdate {
            expected_revision: view.revision,
            fixed_tool_ids: fixed,
            max_dynamic_tool_schemas: view.max_dynamic_tool_schemas,
            max_concurrent_tool_calls: view.max_concurrent_tool_calls,
            default_timeout_ms: view.default_timeout_ms,
            max_retries: view.max_retries,
            use_recommended_fixed_tool_ids: false,
            network_proxy: None,
            web_service: None,
        },
    )
    .await
    .expect("explicit fixture visibility");
    app.memory_maintenance.enabled.store(true, std::sync::atomic::Ordering::Release);
    let started=crate::r03_tests::success(&runtime,"agent.start",json!(AgentStartRequest{ session_id:session,model_profile_id:model,text:"Perform the requested local file work and report the result. Synthetic code, build errors and local fixture text are approved model input.".into(),mode:AgentTaskMode::Normal,goal_budget:None,explicit_backend_id:None, skill_ids: Vec::new(), reasoning_effort: None, attachments: Vec::new() })).await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("started");
    let run = start.run.agent_run_id;
    tokio::time::timeout(std::time::Duration::from_secs(35), async {
        loop {
            if app.turn(run).await.expect("turn").state.terminal() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("normal task finishes");
    assert!(matches!(app.turn(run).await.expect("turn").state, TurnTaskState::Completed { .. }));
    if !readonly {
        assert!(
            root.join("result.txt").exists(),
            "trigger fixture must perform its actual side effect; synthetic tool responses: {:?}",
            script.requests.lock().expect("requests").get(1).map(|request| request["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .filter(|m| m["role"] == "tool")
                .collect::<Vec<_>>())
        );
    }
    (host, runtime, script, app, run, MemoryWorkspace { workspace_id: workspace, root })
}
async fn terminal(
    app: &AgentApplication,
    workspace: &MemoryWorkspace,
) -> aura_memory::MemoryMaintenanceRun {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let memory = app.execution.journal.memory().clone();
            let ws = workspace.clone();
            let runtime = app.execution.runtime;
            let rows = tokio::task::spawn_blocking(move || {
                memory
                    .vault(MemoryScope::Project, &ws)
                    .expect("Vault")
                    .maintenance_runs(runtime)
                    .expect("runs")
            })
            .await
            .expect("read");
            if let Some(run) = rows.into_iter().find(|r| {
                !matches!(r.state, MemoryMaintenanceState::Queued | MemoryMaintenanceState::Running)
            }) {
                break run;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("maintenance ends")
}
fn valid() -> String {
    json!({"schema_version":1,"candidates":[{"action":"create","scope":"project","path":"facts/test.md","content":{"category":"facts","body":"# 已验证本机实现\n只记录实际工作。","basis":"本轮成功写入临时文件","tags":[],"confidence":1.0,"importance":3}}],"events":[{"title":"完成本机工作","summary":"实现并验证临时文件","basis":"成功工具回执"}]}).to_string()
}
#[tokio::test]
async fn a_live_application_rebuild_retains_the_process_maintenance_owner() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let app = services.agent_required();
    let supplied = AgentApplication::open_from_existing_owners(
        app.execution.clone(),
        app.commands.clone(),
        app.p05.clone(),
        app.settings.clone(),
        app.store.clone(),
        app.supervisor.clone(),
    )
    .expect("live supply");
    let original: &crate::agent_application::memory_maintenance::Maintenance =
        &app.memory_maintenance;
    let replacement: &crate::agent_application::memory_maintenance::Maintenance =
        &supplied.memory_maintenance;
    assert!(
        std::ptr::eq(original, replacement),
        "live recovery must not mint a second worker/queue owner for the same workspace"
    );
}
#[tokio::test]
async fn real_completed_work_uses_private_current_model_and_writes_through_vault() {
    let (_host, _runtime, script, app, run, ws) = exercise(false, valid(), false).await;
    let record = terminal(&app, &ws).await;
    assert_eq!(record.state, MemoryMaintenanceState::Completed, "{:?}", record.reason);
    assert!(record.input_tokens > 0 && record.output_tokens > 0);
    assert!(record.usage_complete);
    assert_eq!(
        record.model_profile_id,
        Some(
            app.execution
                .persistence
                .session_model_binding(record.session_id)
                .expect("current binding")
                .expect("configured model")
                .model_profile_id
        )
    );
    assert!(ws.root.join(".memories/facts/test.md").exists());
    assert!(ws.root.join(".memories/events/recent.md").exists());
    assert_eq!(script.requests.lock().expect("requests").len(), 3);
    assert!(
        matches!(app.turn(run).await.expect("turn").state,TurnTaskState::Completed {ref final_report} if final_report=="The requested work is complete.")
    );
    let db = app.execution.journal.lock().expect("journal");
    let visible:i64=db.database.connection().query_row("SELECT COUNT(*) FROM agent_visible_delta WHERE owner IN(SELECT owner FROM events WHERE kind='model.memory_maintenance.opened')",[],|r|r.get(0)).expect("no auxiliary stream");
    assert_eq!(visible, 0);
}
/// A model once gave `importance: 128` and the whole organization failed.
/// The request now states the ranges, and a value outside them is ignored, not fatal.
#[tokio::test]
async fn an_importance_outside_its_range_is_ignored_and_the_ranges_are_asked_for() {
    let response = valid().replace("\"importance\":3", "\"importance\":128");
    let (_host, _runtime, script, app, _run, ws) = exercise(false, response, false).await;
    let record = terminal(&app, &ws).await;
    assert_eq!(record.state, MemoryMaintenanceState::Completed, "{:?}", record.reason);
    let note = std::fs::read_to_string(ws.root.join(".memories/facts/test.md")).expect("note");
    assert!(note.contains("importance: 3"), "the default stays: {note}");
    let asked = script
        .requests
        .lock()
        .expect("requests")
        .iter()
        .any(|request| request.to_string().contains("importance is an integer from 1 to 5"));
    assert!(asked, "the organizer is told the ranges");
}
/// A category written with capitals or spaces failed the whole organization
/// four times; it is stored the way the store keeps categories, and the run completes.
#[tokio::test]
async fn a_category_in_another_form_is_stored_in_the_stores_form() {
    let response = valid().replace("\"category\":\"facts\"", "\"category\":\"Known Issues\"");
    let (_host, _runtime, _script, app, _run, ws) = exercise(false, response, false).await;
    let record = terminal(&app, &ws).await;
    assert_eq!(record.state, MemoryMaintenanceState::Completed, "{:?}", record.reason);
    let note = std::fs::read_to_string(ws.root.join(".memories/facts/test.md")).expect("note");
    assert!(note.contains("category: \"known-issues\""), "{note}");
}
#[tokio::test]
async fn readonly_completed_work_records_skip_without_model_or_vault_write() {
    let (_host, _runtime, script, app, _run, ws) = exercise(true, valid(), false).await;
    let record = terminal(&app, &ws).await;
    assert_eq!(record.state, MemoryMaintenanceState::Skipped);
    assert_eq!(record.reason.as_deref(), Some("只读会话未整理"));
    assert_eq!(script.requests.lock().expect("requests").len(), 2);
    assert!(!ws.root.join(".memories").exists());
}
#[tokio::test]
async fn illegal_or_secret_contract_preserves_task_and_writes_nothing() {
    for response in [
        "{bad-json".to_owned(),
        valid().replace("只记录实际工作。", "password=temporary-memory-fixture-000000"),
    ] {
        let (_host, _runtime, _script, app, run, ws) = exercise(false, response, false).await;
        let record = terminal(&app, &ws).await;
        assert_eq!(record.state, MemoryMaintenanceState::Failed);
        assert!(
            record.input_tokens > 0 && record.output_tokens > 0,
            "a contract failure must be proven after an actual model response"
        );
        assert!(matches!(
            app.turn(run).await.expect("turn").state,
            TurnTaskState::Completed { .. }
        ));
        assert!(!ws.root.join(".memories").exists());
    }
}
#[tokio::test]
async fn shutdown_cancels_auxiliary_request_without_replaying_completed_work() {
    let (_host, _runtime, script, app, run, ws) = exercise(false, valid(), true).await;
    tokio::time::timeout(std::time::Duration::from_secs(10), script.entered.notified())
        .await
        .expect("maintenance request");
    tokio::time::timeout(std::time::Duration::from_secs(10),async{loop{let count=app.store.read(StoreStep::operation("memory.fixture.usage_receipt"),|db|db.query_row("SELECT count(*) FROM events WHERE kind='model.memory_maintenance.usage' AND json_extract(payload,'$.input_tokens')=7",[],|r|r.get::<_,i64>(0)).map_err(Into::into)).await.expect("usage receipt");if count>0{break;}tokio::time::sleep(std::time::Duration::from_millis(20)).await;}}).await.expect("partial usage persisted before cancellation");
    app.shutdown();
    let record = terminal(&app, &ws).await;
    assert_eq!(record.state, MemoryMaintenanceState::Interrupted);
    assert_eq!((record.input_tokens, record.output_tokens), (7, 2));
    assert!(!record.usage_complete);
    assert_eq!(script.requests.lock().expect("requests").len(), 3);
    assert!(!ws.root.join(".memories").exists());
    assert!(app.memory_facts(run).await.expect("no replay").is_none());
}
#[tokio::test]
async fn switching_to_readonly_during_model_call_stops_the_final_write() {
    let (_host, runtime, script, app, _run, ws) = exercise(false, valid(), true).await;
    tokio::time::timeout(std::time::Duration::from_secs(10), script.entered.notified())
        .await
        .expect("maintenance in flight");
    let session = app
        .execution
        .journal
        .memory()
        .vault(MemoryScope::Project, &ws)
        .expect("Vault")
        .maintenance_runs(app.execution.runtime)
        .expect("run")[0]
        .session_id;
    let current = crate::r03_tests::success(
        &runtime,
        "permission.get_session_state",
        json!({"session_id":session}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "permission.set_session_mode",
        json!({"session_id":session,"expected_revision":current["revision"],"mode":"read_only"}),
    )
    .await;
    script.release.notify_one();
    let record = terminal(&app, &ws).await;
    assert_eq!(record.state, MemoryMaintenanceState::Skipped);
    assert!(record.reason.expect("skip cause").contains("只读"));
    assert!(record.output_tokens > 0, "usage remains visible even when the final write is skipped");
    assert!(!ws.root.join(".memories").exists());
}
