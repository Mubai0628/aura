//! An entry of a zip in the conversation's workspace opens as one outside
//! it does; a file outside the workspace that a message names opens once that message is found to
//! name it, and not otherwise.
#![allow(clippy::unwrap_used)]
use super::*;
use std::io::Write as _;

async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

/// The refusal a route gave, as text; a route that answered is a failed test.
async fn refused(runtime: &crate::RuntimeState, method: &str, params: Value) -> String {
    let response = Box::pin(crate::r03_tests::attempt(runtime, method, params)).await.unwrap();
    assert!(response.get("error").is_some(), "{method} answered: {response}");
    response["error"].to_string()
}

fn write_zip(path: &std::path::Path, entries: &[(&str, &str)]) {
    let mut zip = ::zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, text) in entries {
        zip.start_file(*name, ::zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
}

#[tokio::test]
async fn an_entry_of_a_zip_in_the_workspace_opens_and_follows_the_zip() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, _model) = prepare(&host, &provider).await;
    let workspace = host.root.path().join("waiting-workspace");
    std::fs::create_dir_all(workspace.join("资料")).unwrap();
    let zip = workspace.join("资料").join("课程.zip");
    write_zip(&zip, &[("课程/说明.md", "第一版")]);
    let entry =
        json!({"session_id": session, "workspace_path": "资料/课程.zip", "entry": "课程/说明.md"});
    let opened = call(&runtime, "agent.attachments.entry", entry.clone()).await;
    assert_eq!(std::fs::read_to_string(opened["path"].as_str().unwrap()).unwrap(), "第一版");

    // The zip replaced at the same place is copied from again, not taken from the old copy.
    std::fs::remove_file(&zip).unwrap();
    write_zip(&zip, &[("课程/说明.md", "第二版")]);
    let again = call(&runtime, "agent.attachments.entry", entry).await;
    assert_eq!(std::fs::read_to_string(again["path"].as_str().unwrap()).unwrap(), "第二版");

    let outside = refused(
        &runtime,
        "agent.attachments.entry",
        json!({"session_id": session, "workspace_path": "../课程.zip", "entry": "课程/说明.md"}),
    )
    .await;
    assert!(outside.contains("agent.attachment_missing"), "{outside}");
    let neither = refused(
        &runtime,
        "agent.attachments.entry",
        json!({"session_id": session, "entry": "课程/说明.md"}),
    )
    .await;
    assert!(neither.contains("agent.attachment_entry_invalid"), "{neither}");
}

#[tokio::test]
async fn a_file_outside_the_workspace_opens_only_from_the_message_that_names_it() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    call(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    let folder = host.root.path().join("外部");
    std::fs::create_dir_all(&folder).unwrap();
    let named = folder.join("报告.txt");
    std::fs::write(&named, "外部报告").unwrap();
    let other = folder.join("别的.txt");
    std::fs::write(&other, "没提到").unwrap();
    let agent = match &runtime.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("ready"),
    };
    let database = agent.execution.journal.database_path().unwrap();
    call(
        &runtime,
        "agent.submit",
        json!({"session_id": session, "destination": {"lane": "next_turn"},
            "text": format!("看看 `{}` 里写了什么；库在 {}", named.display(), database.display())}),
    )
    .await;
    let event = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let history = call(
                &runtime,
                "agent.history",
                json!({"session_id":session,"before_turn_task_id":null,"within_turn":null,"limit":10}),
            )
            .await;
            if let Some(id) = history["turns"][0]["user_messages"][0]["event_id"].as_str() {
                break id.to_owned();
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the message is in the history");
    let source = json!({"kind": "user", "event_id": event});
    let ask = |path: &std::path::Path| json!({"session_id": session, "path": path.display().to_string(), "mentioned_in": source});

    let located = call(&runtime, "agent.files.mentioned", ask(&named)).await;
    assert_eq!(
        std::fs::canonicalize(located["path"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&named).unwrap()
    );
    assert_eq!(located["byte_length"], "外部报告".len());
    let unnamed = refused(&runtime, "agent.files.mentioned", ask(&other)).await;
    assert!(unnamed.contains("agent.mentioned_file_not_mentioned"), "{unnamed}");
    let own = refused(&runtime, "agent.files.mentioned", ask(&database)).await;
    assert!(own.contains("agent.mentioned_file_protected"), "{own}");

    // Another conversation cannot use this message as its own.
    let created = call(
        &runtime,
        "workspace.create",
        json!({"selected_root": folder, "display_name": "Other"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).unwrap();
    let stranger = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let elsewhere = refused(
        &runtime,
        "agent.files.mentioned",
        json!({"session_id": stranger, "path": named.display().to_string(), "mentioned_in": source}),
    )
    .await;
    assert!(elsewhere.contains("agent.mentioned_file_not_mentioned"), "{elsewhere}");
}
