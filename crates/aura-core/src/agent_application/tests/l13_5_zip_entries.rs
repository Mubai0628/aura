//! An entry of a zip a
//! message carried from outside the workspace opens for its conversation. Core reads the zip
//! through the conversation's read-only authorization and copies that one entry into Aura's own
//! folder for the conversation, which goes when the conversation is deleted.
#![allow(clippy::unwrap_used)]
use super::*;
use std::io::Write as _;

const NOTES: &str = "# 说明\n第二行";

/// The refusal a route gave, as text; a route that answered is a failed test.
async fn refused(runtime: &crate::RuntimeState, method: &str, params: Value) -> String {
    let response = Box::pin(crate::r03_tests::attempt(runtime, method, params)).await.unwrap();
    assert!(response.get("error").is_some(), "{method} answered: {response}");
    response["error"].to_string()
}

async fn call(runtime: &crate::RuntimeState, method: &str, params: Value) -> Value {
    Box::pin(crate::r03_tests::success(runtime, method, params)).await
}

fn write_zip(path: &std::path::Path) {
    let mut zip = ::zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    let options = ::zip::write::SimpleFileOptions::default();
    zip.add_directory("项目/", options).unwrap();
    zip.start_file("项目/说明.md", options).unwrap();
    zip.write_all(NOTES.as_bytes()).unwrap();
    zip.start_file("项目/src/main.c", options).unwrap();
    zip.write_all(b"int main(void) { return 0; }\n").unwrap();
    zip.finish().unwrap();
}

#[tokio::test]
async fn an_entry_of_an_attached_zip_opens_for_its_conversation_only() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    call(
        &runtime,
        "models.session.set_binding",
        json!({"expected_revision": "0", "session_id": session, "model_profile_id": model}),
    )
    .await;
    let zip = host.root.path().join("课程设计.zip");
    write_zip(&zip);
    call(
        &runtime,
        "agent.submit",
        json!({"session_id": session, "text": "压缩包里都有什么文件", "destination": {"lane": "next_turn"},
            "attachments": [{"kind": "file", "path": zip}]}),
    )
    .await;
    let agent = match &runtime.startup_mode {
        crate::StartupMode::Ready(services) => services.agent_required(),
        _ => panic!("ready"),
    };
    let grant = tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            if let Some(grant) =
                agent.execution.persistence.list_session_external_grants(session).unwrap().first()
            {
                break grant.id;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the zip is attached");
    let entry =
        |name: &str| json!({"session_id": session, "external_grant_id": grant, "entry": name});

    let opened = call(&runtime, "agent.attachments.entry", entry("项目/说明.md")).await;
    let copy = std::path::PathBuf::from(opened["path"].as_str().unwrap());
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), NOTES);
    assert_eq!(opened["byte_length"], NOTES.len());
    assert_eq!(copy.file_name().unwrap(), "说明.md");
    assert!(!opened["path"].as_str().unwrap().starts_with(r"\\?\"), "{opened}");
    // Opened again, it is the same copy.
    let again = call(&runtime, "agent.attachments.entry", entry("项目/说明.md")).await;
    assert_eq!(again["path"], opened["path"]);

    let folder = refused(&runtime, "agent.attachments.entry", entry("项目/")).await;
    assert!(folder.contains("agent.attachment_entry"), "{folder}");
    let missing = refused(&runtime, "agent.attachments.entry", entry("项目/没有.md")).await;
    assert!(missing.contains("agent.attachment_entry"), "{missing}");
    let other = host.root.path().join("其他");
    std::fs::create_dir(&other).unwrap();
    let created = call(
        &runtime,
        "workspace.create",
        json!({"selected_root": other, "display_name": "Other"}),
    )
    .await;
    let workspace: aura_workspace_domain::WorkspaceSummary =
        serde_json::from_value(created["workspace"].clone()).unwrap();
    let stranger = create_session(&runtime, workspace.workspace_id, PermissionMode::ReadOnly).await;
    let elsewhere = refused(
        &runtime,
        "agent.attachments.entry",
        json!({"session_id": stranger, "external_grant_id": grant, "entry": "项目/说明.md"}),
    )
    .await;
    assert!(elsewhere.contains("agent.attachment_missing"), "{elsewhere}");

    // Deleting the conversation takes the copy with it.
    wait_idle(&runtime, session).await;
    call(&runtime, "session.archive", json!({"session_id": session})).await;
    call(&runtime, "session.delete", json!({"session_id": session})).await;
    assert!(!copy.exists(), "{}", copy.display());
}

/// Waits until the conversation's turn has completed.
async fn wait_idle(runtime: &crate::RuntimeState, session: SessionId) {
    tokio::time::timeout(std::time::Duration::from_secs(45), async {
        loop {
            let history = call(
                runtime,
                "agent.history",
                json!({"session_id":session,"before_turn_task_id":null,"within_turn":null,"limit":10}),
            )
            .await;
            if history["turns"][0]["turn"]["state"]["state"] == "completed" {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("the turn ends");
}
