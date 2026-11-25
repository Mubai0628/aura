use super::*;
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
    async fn new(response: &'static str, park: bool) -> Self {
        Self::with_count(response, park, 2).await
    }
    async fn with_count(response: &'static str, park: bool, count: usize) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("loopback");
        let url = format!("http://{}/v1", listener.local_addr().expect("address"));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let saved = requests.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let notice = entered.clone();
        let release = Arc::new(tokio::sync::Notify::new());
        let unblock = release.clone();
        let task = tokio::spawn(async move {
            for ordinal in 0..count {
                let (mut socket, _) = listener.accept().await.expect("request");
                let mut raw = vec![];
                let mut buffer = [0; 8192];
                let body = loop {
                    let n = socket.read(&mut buffer).await.expect("read");
                    assert!(n > 0 && raw.len() < 2 * 1024 * 1024);
                    raw.extend_from_slice(&buffer[..n]);
                    if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&raw[..end]).to_ascii_lowercase();
                        assert!(
                            !headers.contains("authorization:") && !headers.contains("cookie:")
                        );
                        let length = headers
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .expect("content length")
                            .trim()
                            .parse::<usize>()
                            .expect("number");
                        if raw.len() >= end + 4 + length {
                            break serde_json::from_slice::<Value>(&raw[end + 4..end + 4 + length])
                                .expect("JSON");
                        }
                    }
                };
                saved.lock().expect("requests").push(body.clone());
                if ordinal > 0 {
                    assert!(body.to_string().contains("Maintain a concise role description"));
                    assert!(!body["tools"].as_array().is_some_and(|tools| !tools.is_empty()));
                    notice.notify_one();
                    if park {
                        unblock.notified().await;
                    }
                }
                let content =
                    if ordinal == 0 { "已按用户要求完成检查。" } else { response };
                let chunk = json!({"id":format!("profile-{ordinal}"),"object":"chat.completion.chunk","model":"fixture","choices":[{"index":0,"delta":{"role":"assistant","content":content},"finish_reason":"stop"}],"usage":{"prompt_tokens":17,"completion_tokens":9,"total_tokens":26}});
                let body = format!("data: {chunk}\n\ndata: [DONE]\n\n");
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                socket.write_all(head.as_bytes()).await.expect("headers");
                socket.write_all(body.as_bytes()).await.expect("body");
                socket.shutdown().await.expect("end");
            }
        });
        Self { url, requests, entered, release, task }
    }
}

#[tokio::test]
async fn session_agent_profile_compensation_does_not_hide_database_failure() {
    use aura_agent_collaboration::{AgentProfileGeneration, AgentProfileGenerationState, profile};
    use aura_persistence_core::PersistenceCoreError as Error;

    let script = Script::with_count("unused auxiliary response", false, 1).await;
    let host = crate::r03_tests::Fixture::new();
    let (runtime, session) = setup(&host, &script, false).await;
    call(
        &runtime,
        "agent.team.profile.set",
        json!({
            "session_id":session,"expected_revision":0,"description":"故障时也保留用户职责"
        }),
    )
    .await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let app = services.agent_required();
    let key = format!("preview:{}", ModelStepId::new());
    let reserved = key.clone();
    app.store.write(StoreStep::operation("test.profile.reject_compensation"), move |journal| {
        journal.database.unit_of_work(|tx| {
            let (run,turn): (String,String) = tx.query_row(
                "SELECT r.id,t.id FROM agent_runs r JOIN turn_tasks t ON t.id=r.turn_task_id WHERE t.session_id=?1 AND t.state='completed'",
                [session.to_string()], |row|Ok((row.get(0)?,row.get(1)?)),
            )?;
            let record = AgentProfileGeneration {
                source_run_id:run.parse().map_err(|_|Error::Invalid("source run"))?,
                source_turn_id:turn.parse().map_err(|_|Error::Invalid("source turn"))?,
                model_profile_id:None,state:AgentProfileGenerationState::Running,
                input_tokens:11,output_tokens:3,usage_complete:false,message:None,preview:None,
            };
            assert!(profile::reserve_generation_as(tx,&reserved,session,1,&record,UtcTimestamp::now())?);
            tx.execute_batch("CREATE TRIGGER test_profile_reject_compensation BEFORE UPDATE OF state ON agent_collaboration_generations BEGIN SELECT RAISE(ABORT,'fixture.profile_failure_write'); END;")?;
            Ok(())
        })
    }).await.expect("durable preview and controlled SQLite failure");

    let failed = app.profile_failure_outcome_for_test(key.clone()).await;
    assert!(
        failed.starts_with("Err("),
        "compensation must propagate the real SQLite failure, got {failed}"
    );
    assert!(failed.contains("fixture.profile_failure_write"), "{failed}");
    let unchanged = app.agent_profile(session).await.expect("old profile remains readable");
    assert_eq!(unchanged.description, "故障时也保留用户职责");
    assert_eq!(
        unchanged.generation.expect("unsettled record").state,
        AgentProfileGenerationState::Running
    );

    app.store
        .write(StoreStep::operation("test.profile.repair_compensation"), |journal| {
            journal.database.unit_of_work(|tx| {
                tx.execute_batch("DROP TRIGGER test_profile_reject_compensation;")?;
                Ok(())
            })
        })
        .await
        .expect("remove only the fixture's fault");
    assert_eq!(app.profile_failure_outcome_for_test(key).await, "Ok(())");
    let settled = app.agent_profile(session).await.expect("settled profile");
    assert_eq!(settled.description, "故障时也保留用户职责");
    assert_eq!((settled.input_tokens, settled.output_tokens), (11, 3));
    assert!(!settled.usage_complete);
    assert_eq!(
        settled.generation.expect("failed record").state,
        AgentProfileGenerationState::Failed
    );
    assert_eq!(
        script.requests.lock().expect("requests").len(),
        1,
        "compensation never replays a model request"
    );
}

async fn start(script: &Script) -> (crate::r03_tests::Fixture, crate::RuntimeState, SessionId) {
    let host = crate::r03_tests::Fixture::new();
    let (runtime, session) = setup(&host, script, true).await;
    (host, runtime, session)
}

async fn setup(
    host: &crate::r03_tests::Fixture,
    script: &Script,
    maintenance: bool,
) -> (crate::RuntimeState, SessionId) {
    let runtime = host.start().await;
    let model = profile(&runtime, &script.url).await;
    let root = host.root.path().join("profile-workspace");
    std::fs::create_dir(&root).expect("workspace");
    let created = call(
        &runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":"Identity fixture"}),
    )
    .await;
    let workspace =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace ID");
    let session = create_session(&runtime, workspace, PermissionMode::ReadOnly).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let binding = services.persistence.session_model_binding_state(session).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":session,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    services
        .agent_required()
        .profile_maintenance
        .enabled
        .store(maintenance, std::sync::atomic::Ordering::Release);
    turn(
        &runtime,
        session,
        model,
        "请检查测试职责边界并给出结论，只使用这条模拟输入；本地模型夹具没有真实凭据。",
    )
    .await;
    (runtime, session)
}
async fn finished(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        loop {
            let value =
                call(runtime, "agent.team.profile.get", json!({"session_id":session})).await;
            if value["generation"]["state"].as_str().is_some_and(|state| state != "running") {
                return value;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("profile generation completes without reopening the user turn")
}

#[tokio::test]
async fn session_agent_generates_profile_through_real_auxiliary_model_and_records_usage() {
    let script = Script::new(r#"{"description":"负责测试和回归验证"}"#, false).await;
    let (_host, runtime, session) = start(&script).await;
    let value = finished(&runtime, session).await;
    assert_eq!(value["description"], "负责测试和回归验证");
    assert_eq!(value["description_source"], "model");
    assert_eq!(value["generation"]["state"], "completed");
    assert_eq!(value["input_tokens"], 17);
    assert_eq!(value["output_tokens"], 9);
    assert_eq!(value["usage_complete"], true);
    assert_eq!(script.requests.lock().expect("requests").len(), 2, "no recursive maintenance");
}

#[tokio::test]
async fn session_agent_invalid_generated_text_does_not_reopen_or_fail_the_finished_turn() {
    let script = Script::new("not valid structured output", false).await;
    let (_host, runtime, session) = start(&script).await;
    let value = finished(&runtime, session).await;
    assert_eq!(value["description"], "");
    assert_eq!(value["generation"]["state"], "failed");
    assert_eq!(value["input_tokens"], 17);
    assert_eq!(value["usage_complete"], true);
    assert_eq!(script.requests.lock().expect("requests").len(), 2);
}

#[tokio::test]
async fn session_agent_model_reply_cannot_overwrite_a_user_edit_made_while_it_was_generating() {
    let script = Script::new(r#"{"description":"迟到的职责"}"#, true).await;
    let (_host, runtime, session) = start(&script).await;
    tokio::time::timeout(std::time::Duration::from_secs(30), script.entered.notified())
        .await
        .expect("auxiliary request");
    let current = call(&runtime, "agent.team.profile.get", json!({"session_id":session})).await;
    call(&runtime,"agent.team.profile.set",json!({"session_id":session,"expected_revision":current["revision"],"description":"用户选择的职责"})).await;
    script.release.notify_one();
    let value = finished(&runtime, session).await;
    assert_eq!(value["description"], "用户选择的职责");
    assert_eq!(value["description_source"], "user");
    assert_eq!(value["generation"]["state"], "superseded");
    assert_eq!(value["input_tokens"], 17);
}

#[tokio::test]
async fn session_agent_manual_regeneration_is_a_preview_until_user_adopts_it() {
    let script = Script::with_count(r#"{"description":"模型生成的建议"}"#, false, 3).await;
    let (_host, runtime, session) = start(&script).await;
    let initial = finished(&runtime, session).await;
    let edited = call(&runtime, "agent.team.profile.set", json!({"session_id":session,"expected_revision":initial["revision"],"description":"用户自己保留的简介"})).await;
    call(
        &runtime,
        "agent.team.profile.preview",
        json!({"session_id":session,"expected_revision":edited["revision"]}),
    )
    .await;
    let preview = finished(&runtime, session).await;
    assert_eq!(preview["description"], "用户自己保留的简介");
    assert_eq!(preview["revision"], edited["revision"]);
    assert_eq!(preview["generation"]["preview"]["description"], "模型生成的建议");
    assert_eq!(preview["generation"]["preview"]["base_revision"], edited["revision"]);
    assert_eq!(preview["input_tokens"], 34);
    assert_eq!(script.requests.lock().expect("requests").len(), 3);
    let adopted = call(&runtime, "agent.team.profile.set", json!({"session_id":session,"expected_revision":edited["revision"],"description":preview["generation"]["preview"]["description"]})).await;
    assert_eq!(adopted["description"], "模型生成的建议");
    assert_eq!(adopted["description_source"], "user");
}

#[tokio::test]
async fn session_agent_startup_interrupts_preview_without_replaying_or_reopening_the_turn() {
    use aura_agent_collaboration::{AgentProfileGeneration, AgentProfileGenerationState, profile};
    use aura_persistence_core::PersistenceCoreError as Error;

    let host = Arc::new(crate::r03_tests::Fixture::new());
    let script = Arc::new(Script::with_count("unused auxiliary response", false, 1).await);
    let (previous_host, previous_script) = (host.clone(), script.clone());
    let (session, run, revision) = tokio::task::spawn_blocking(move || {
        let process = tokio::runtime::Builder::new_current_thread().enable_all().build().expect("isolated runtime");
        let result = process.block_on(async move {
            let (runtime, session) = setup(&previous_host, &previous_script, false).await;
            let edited = call(&runtime, "agent.team.profile.set", json!({
                "session_id":session,"expected_revision":0,"description":"用户保留的恢复职责"
            })).await;
            let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
            let run = services.agent_required().store.write(
                StoreStep::operation("test.profile.seed_interrupted_preview"),
                move |journal| journal.database.unit_of_work(|tx| {
                    let (run, turn): (String, String) = tx.query_row(
                        "SELECT r.id,t.id FROM agent_runs r JOIN turn_tasks t ON t.id=r.turn_task_id WHERE t.session_id=?1 AND t.state='completed'",
                        [session.to_string()], |row| Ok((row.get(0)?,row.get(1)?)),
                    )?;
                    let run: AgentRunId = run.parse().map_err(|_| Error::Invalid("source run"))?;
                    let turn: TurnTaskId = turn.parse().map_err(|_| Error::Invalid("source turn"))?;
                    let generation = AgentProfileGeneration {
                        source_run_id: run, source_turn_id: turn, model_profile_id: None,
                        state: AgentProfileGenerationState::Running, input_tokens: 11, output_tokens: 3,
                        usage_complete: false, message: None, preview: None,
                    };
                    assert!(profile::reserve_generation_as(tx, &format!("preview:{}",ModelStepId::new()), session, 1, &generation, UtcTimestamp::now())?);
                    Ok(run)
                }),
            ).await.expect("durable unfinished preview");
            (session, run, edited["revision"].clone())
        });
        process.shutdown_timeout(std::time::Duration::from_secs(2));
        result
    }).await.expect("the previous Core process stops");

    let runtime = host.start().await;
    let recovered = call(&runtime, "agent.team.profile.get", json!({"session_id":session})).await;
    assert_eq!(recovered["description"], "用户保留的恢复职责");
    assert_eq!(recovered["description_source"], "user");
    assert_eq!(recovered["revision"], revision);
    assert_eq!(recovered["generation"]["state"], "interrupted");
    assert_eq!(recovered["input_tokens"], 11);
    assert_eq!(recovered["output_tokens"], 3);
    assert_eq!(recovered["usage_complete"], false);
    assert!(recovered["generation"]["preview"].is_null());
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let (turn, count) = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.profile.recovered_turn"), move |db| {
            let turn = aura_persistence_v2::agent::turns::load(
                db,
                aura_persistence_v2::agent::turns::load_run(db, run)?.turn_task_id,
            )?;
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM turn_tasks WHERE session_id=?1",
                [session.to_string()],
                |row| row.get(0),
            )?;
            Ok((turn, count))
        })
        .await
        .expect("recovered original turn");
    assert!(matches!(turn.state, TurnTaskState::Completed { .. }));
    assert_eq!(count, 1, "no synthetic user turn is created by profile recovery");
    assert_eq!(
        script.requests.lock().expect("requests").len(),
        1,
        "startup does not replay auxiliary model work"
    );
}
