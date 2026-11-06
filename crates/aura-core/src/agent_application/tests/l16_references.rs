//! What a message refers to with "@": the paths typed after "@" in its words and the references
//! chosen beside them. Core resolves them when the message is sent and the model reads them after
//! what was typed, one line per reference, in that message only: every step and every later turn
//! sends the message the same, though the files change, and the instructions and the tool list
//! carry none of it. A chosen path that is not in the workspace is refused with its name, a typed
//! one stays text, and references alone make a message, queued or started.
use super::*;
use provider_fixture::Scenario;

const READ: Scenario = Scenario::Sequence {
    calls: &[(
        "fs.read",
        r#"{"path":"src/lib.rs","range":{"range_kind":"full"},"explicit_encoding":null}"#,
    )],
};

/// What was typed: a path there, and one that is not.
const TYPED: &str = "Look at @src/lib.rs, not @gone.rs";

/// The message as the model reads it: what was typed, then what it refers to.
const SENT: &str = "Look at @src/lib.rs, not @gone.rs\n\nReferred to in this message:\n- \
                    workspace file `src/lib.rs`, not included: read it from the workspace if \
                    the task needs it\n- workspace folder `docs`, not included: list or search \
                    it if the task needs it";

fn path(path: &str) -> Value {
    json!({"kind": "workspace_path", "path": path})
}

fn start_request(session: SessionId, model: ModelProfileId, text: &str, refs: Value) -> Value {
    json!({
        "session_id": session,
        "model_profile_id": model,
        "text": text,
        "mode": "normal",
        "goal_budget": null,
        "references": refs,
    })
}

/// The texts of a request's user messages.
fn users(request: &Value) -> Vec<String> {
    request["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str().map(str::to_owned))
        .collect()
}

async fn history(runtime: &crate::RuntimeState, session: SessionId) -> Value {
    crate::r03_tests::success(
        runtime,
        "agent.history",
        json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
    )
    .await
}

fn workspace(host: &crate::r03_tests::Fixture) -> std::path::PathBuf {
    let root = host.root.path().join("waiting-workspace");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::create_dir_all(root.join("docs")).expect("docs");
    std::fs::write(root.join("src/lib.rs"), "pub fn lib() {}\n").expect("file");
    root
}

#[tokio::test]
async fn references_follow_what_was_typed_the_same_in_every_step_and_turn_and_nowhere_else() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(READ).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let root = workspace(&host);

    // A chosen path that is not in the workspace is refused with its name; one outside it is no
    // workspace path at all. Nothing starts.
    let missing = crate::r03_tests::attempt(
        &runtime,
        "agent.start",
        start_request(session, model, "Look", json!([path("src/main.rs")])),
    )
    .await;
    assert!(format!("{missing:?}").contains("当前工作区里没有 src/main.rs"), "{missing:?}");
    let outside = crate::r03_tests::attempt(
        &runtime,
        "agent.start",
        start_request(session, model, "Look", json!([path("../elsewhere.txt")])),
    )
    .await;
    assert!(format!("{outside:?}").contains("parent traversal"), "{outside:?}");
    assert!(provider.requests.lock().expect("requests").is_empty(), "nothing was started");

    // The typed ones first, then the chosen ones, each once.
    let run: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.start",
            start_request(session, model, TYPED, json!([path("docs"), path("src\\lib.rs")])),
        )
        .await,
    )
    .expect("run");
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("requests").clone();
    assert!(requests.len() >= 2, "the read and the answer after it");
    for request in &requests {
        assert!(users(request).iter().any(|text| text == SENT), "{request}");
    }
    // In that message only: the instructions and the tool list name none of it.
    for request in &requests {
        let instructions = request["messages"][0]["content"].as_str().expect("instructions");
        assert!(!instructions.contains("Referred to") && !instructions.contains("src/lib.rs"));
        assert!(!request["tools"].to_string().contains("Referred to"));
    }

    // The conversation shows what was typed with what it referred to.
    let shown = history(&runtime, session).await;
    let user = &shown["turns"][0]["user_messages"][0];
    assert_eq!(user["text"], TYPED);
    assert_eq!(
        user["references"],
        json!([{"kind": "file", "label": "src/lib.rs"}, {"kind": "folder", "label": "docs"}])
    );

    // A later turn sends the message as it was sent, though the file is gone now.
    std::fs::remove_file(root.join("src/lib.rs")).expect("removed");
    let next = start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: next.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let last = provider.requests.lock().expect("requests").last().expect("a request").clone();
    assert!(users(&last).iter().any(|text| text == SENT), "{last}");
}

#[tokio::test]
async fn a_queued_message_of_references_alone_starts_its_turn_with_them() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    workspace(&host);
    crate::r03_tests::success(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        json!({"session_id": session, "text": "", "destination": {"lane": "next_turn"}, "references": [path(".")]}),
    )
    .await;
    let first = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            if let Some(first) = provider.requests.lock().expect("requests").first() {
                break first.clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the queued message started a turn");
    let alone = "Referred to in this message:\n- the workspace root folder `.`, not included: list \
                 or search it if the task needs it";
    assert!(users(&first).iter().any(|text| text == alone), "{first}");
    let turn = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let shown = history(&runtime, session).await;
            if shown["turns"][0]["turn"]["state"]["state"] == "completed" {
                break shown["turns"][0].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the turn completed");
    assert_eq!(turn["user_messages"][0]["text"], "");
    assert_eq!(turn["user_messages"][0]["references"], json!([{"kind": "folder", "label": "."}]));
}

#[tokio::test]
async fn a_typed_path_git_ignores_is_a_reference_and_one_through_a_link_outside_is_text() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let root = workspace(&host);
    std::fs::write(root.join(".gitignore"), "build/\n").expect("ignore rules");
    std::fs::create_dir_all(root.join("build")).expect("build");
    std::fs::write(root.join("build/out.txt"), "built\n").expect("built file");
    let outside = host.root.path().join("outside");
    std::fs::create_dir_all(&outside).expect("outside");
    std::fs::write(outside.join("secret.txt"), "secret\n").expect("outside file");
    junction::create(&outside, root.join("link")).expect("a link to outside the workspace");

    let text = "Check @build/out.txt and @link/secret.txt";
    let run: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(
            &runtime,
            "agent.start",
            start_request(session, model, text, json!([])),
        )
        .await,
    )
    .expect("run");
    let request = AgentRunRequest { session_id: session, agent_run_id: run.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let first = provider.requests.lock().expect("requests")[0].clone();
    let sent = format!(
        "{text}\n\nReferred to in this message:\n- workspace file `build/out.txt`, not included: \
         read it from the workspace if the task needs it"
    );
    assert!(users(&first).iter().any(|user| user == &sent), "{first}");
}
