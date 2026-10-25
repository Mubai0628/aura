//! The model reads the message with its attachments; the
//! conversation shows what the user typed, with the attachments beside it.
use super::*;

/// A PNG's first bytes: enough for Core to recognise it, nothing decodes them.
const PNG_BASE64: &str = "iVBORw0KGgoAAAAN";

/// The fixture's profile, told whether its model takes images.
async fn images(runtime: &crate::RuntimeState, model: ModelProfileId, on: bool) {
    let success = crate::r03_tests::success;
    let revision =
        success(runtime, "models.settings.get_snapshot", json!({})).await["models_revision"]
            .clone();
    success(
        runtime,
        "models.profile.update",
        json!({"expected_revision":revision,"model_profile_id":model,"display_name":"Fixture",
            "notes":null,"enabled":true,"request_defaults":{"max_output_tokens":4096,
            "temperature":null,"top_p":null,"chat_output_token_field":null,
            "chat_instruction_role":null,"image_input":on},
            "historical_snapshot_max_tokens":1024}),
    )
    .await;
}

async fn bind(runtime: &crate::RuntimeState, session: SessionId, model: ModelProfileId) {
    crate::r03_tests::success(
        runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
}

fn message(session: SessionId, attachments: Value) -> Value {
    json!({"session_id": session, "text": "看看这些", "destination": {"lane": "next_turn"},
        "attachments": attachments})
}

#[tokio::test]
async fn a_picture_and_files_reach_the_model_with_their_message_and_show_beside_it() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    images(&runtime, model, true).await;
    bind(&runtime, session, model).await;
    let docs = host.root.path().join("waiting-workspace").join("docs");
    std::fs::create_dir_all(&docs).expect("docs");
    std::fs::write(docs.join("spec.pdf"), b"%PDF-1.7 fixture").expect("file");
    crate::r03_tests::success(
        &runtime,
        "agent.submit",
        message(
            session,
            json!([
                {"kind": "image", "name": "shot.png", "media_type": "image/png", "data_base64": PNG_BASE64},
                {"kind": "text", "name": "notes.md", "text": "第一行\nsecond line"},
                {"kind": "file", "path": docs.join("spec.pdf")}
            ]),
        ),
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
    .unwrap_or_else(|_| {
        let crate::StartupMode::Ready(s) = &runtime.startup_mode else { panic!("not ready") };
        let journal = s.broker.lock().expect("journal");
        let mut query = journal
            .database
            .connection()
            .prepare("SELECT kind, substr(payload, 1, 600) FROM events WHERE kind LIKE '%error%' OR kind LIKE '%failure%' OR kind LIKE 'agent.turn%' ORDER BY sequence DESC LIMIT 12")
            .expect("query");
        let rows = query
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .expect("rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("rows");
        panic!("the message started no request: {rows:#?}");
    });
    let user = first["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .cloned()
        .expect("the user message");
    let text = user["content"][0]["text"].as_str().expect("its text");
    assert!(text.starts_with("看看这些\n\nAttached to this message:\n"), "{text}");
    assert!(text.contains("- picture `shot.png` (image/png, 12 B), included right after this text as an image you can see"), "{text}");
    assert!(text.contains("- workspace file `docs/spec.pdf` (16 B), not included"), "{text}");
    assert!(text.ends_with("`notes.md`:\n```\n第一行\nsecond line\n```"), "{text}");
    assert_eq!(
        user["content"][1],
        json!({"type": "text", "text": "Picture `shot.png` attached by the user:"})
    );
    assert_eq!(
        user["content"][2],
        json!({"type": "image_url", "image_url": {"url": format!("data:image/png;base64,{PNG_BASE64}")}})
    );

    // The conversation shows what the user typed, with the attachments beside it; the picture is
    // read back for it.
    let shown = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let shown = crate::r03_tests::success(
                &runtime,
                "agent.history",
                json!({"session_id": session, "before_turn_task_id": null, "within_turn": null, "limit": 10}),
            )
            .await;
            if shown["turns"][0]["user_messages"][0].is_object() {
                break shown["turns"][0]["user_messages"][0].clone();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("shown");
    assert_eq!(shown["text"], "看看这些");
    assert_eq!(shown["total_bytes"], 12);
    let attached = shown["attachments"].as_array().expect("attachments");
    assert_eq!(
        attached.iter().map(|a| a["kind"].as_str().unwrap_or_default()).collect::<Vec<_>>(),
        ["image", "text", "file"]
    );
    assert_eq!(attached[2]["path"], "docs/spec.pdf");
    let picture = crate::r03_tests::success(
        &runtime,
        "browser.screenshot.read",
        json!({"session_id": session, "artifact_id": attached[0]["artifact_id"]}),
    )
    .await;
    assert_eq!(picture["data_base64"], PNG_BASE64);
}

#[tokio::test]
async fn pictures_for_a_model_without_images_or_with_the_wrong_type_are_refused() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    images(&runtime, model, false).await;
    bind(&runtime, session, model).await;
    let picture = crate::r03_tests::attempt(
        &runtime,
        "agent.submit",
        message(
            session,
            json!([{"kind": "image", "name": "shot.png", "media_type": "image/png", "data_base64": PNG_BASE64}]),
        ),
    )
    .await;
    assert!(format!("{picture:?}").contains("agent.attachment_images_unsupported"), "{picture:?}");
    // Owner decision 1 replaces the external-file refusal. The selected file's
    // read-only session authority is exercised in l13_3_attachments::external.
    // Not what it says it is: a "PNG" that is a JPEG.
    images(&runtime, model, true).await;
    let mislabeled = crate::r03_tests::attempt(
        &runtime,
        "agent.submit",
        message(
            session,
            json!([{"kind": "image", "name": "a.png", "media_type": "image/png", "data_base64": "/9j/4AAQSkZJRg=="}]),
        ),
    )
    .await;
    assert!(format!("{mislabeled:?}").contains("agent.attachment_image_invalid"), "{mislabeled:?}");
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(provider.requests.lock().expect("requests").is_empty(), "nothing was sent");
    let queue =
        crate::r03_tests::success(&runtime, "agent.inbox", json!({"session_id": session})).await;
    assert_eq!(queue["messages"], json!([]));
}

/// A message that starts a task itself (a Plan or a Goal) carries its attachments the same way.
#[tokio::test]
async fn a_started_task_carries_its_attachments_too() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let started = crate::r03_tests::success(
        &runtime,
        "agent.start",
        json!({"session_id": session, "model_profile_id": model, "text": "", "mode": "normal",
            "goal_budget": null,
            "attachments": [{"kind": "text", "name": "todo.txt", "text": "buy milk"}]}),
    )
    .await;
    let request = AgentRunRequest {
        session_id: session,
        agent_run_id: serde_json::from_value(started["run"]["agent_run_id"].clone()).expect("run"),
    };
    wait(&runtime, &request, |view| matches!(view.turn.state, TurnTaskState::Completed { .. }))
        .await;
    let first = provider.requests.lock().expect("requests").first().cloned().expect("sent");
    assert!(
        first.to_string().contains("Attached to this message:\\n- text file `todo.txt` (8 B)"),
        "{first}"
    );
}
