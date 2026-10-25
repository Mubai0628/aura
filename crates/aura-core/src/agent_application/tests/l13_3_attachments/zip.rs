use super::*;
use ::zip::unstable::write::FileOptionsExt as _;
use std::io::Write as _;

fn archive(path: &std::path::Path, count: usize) {
    let mut zip = ::zip::ZipWriter::new(std::fs::File::create(path).expect("zip file"));
    zip.add_directory("folder/", ::zip::write::SimpleFileOptions::default()).expect("directory");
    for n in 0..count {
        zip.start_file(
            format!("folder/entry-{n:03}.txt"),
            ::zip::write::SimpleFileOptions::default(),
        )
        .expect("entry");
        zip.write_all(b"PRIVATE_ENTRY_BODY_MUST_NOT_BE_SENT").expect("entry bytes");
    }
    zip.finish().expect("zip complete");
}

#[tokio::test]
async fn zip_listing_is_saved_with_the_message_and_truncated_after_two_hundred_entries() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let path = host.root.path().join("waiting-workspace/bundle.zip");
    archive(&path, 202);
    let started = services(&runtime)
        .agent_required()
        .start(CommandInvocationId::new(), start_request(session, model, &path, "list the archive"))
        .await
        .expect("zip accepted");
    finish(&runtime, &started).await;
    let text = request_text(&provider);
    assert!(text.contains("folder/entry-000.txt"), "{text}");
    assert!(text.contains("folder/entry-198.txt"), "{text}");
    assert!(!text.contains("folder/entry-199.txt"), "200 includes the folder entry: {text}");
    assert!(text.contains("另有 3 项"), "{text}");
    assert!(!text.contains("PRIVATE_ENTRY_BODY_MUST_NOT_BE_SENT"));
    let history = crate::r03_tests::success(
        &runtime,
        "agent.history",
        json!({"session_id":session,
        "before_turn_task_id":null,"within_turn":null,"limit":10}),
    )
    .await;
    assert!(
        history["turns"][0]["user_messages"][0]["attachments"][0]["listing_artifact_id"]
            .is_string(),
        "listing must be persisted as user content: {history}"
    );
}

#[tokio::test]
async fn damaged_zip_stays_attached_by_location_with_an_explicit_listing_failure() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let path = host.root.path().join("waiting-workspace/broken.zip");
    std::fs::write(&path, b"not a zip").expect("damaged zip");
    let started = services(&runtime)
        .agent_required()
        .start(CommandInvocationId::new(), start_request(session, model, &path, "look"))
        .await
        .expect("bad zip still accepted by location");
    finish(&runtime, &started).await;
    let text = request_text(&provider);
    assert!(text.contains("broken.zip"));
    assert!(
        text.contains("清单") && text.contains("无法"),
        "explain why only its location is included: {text}"
    );
}

#[tokio::test]
async fn encrypted_zip_entries_and_directories_are_listed_without_password_or_content_reads() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Text).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let path = host.root.path().join("waiting-workspace/encrypted.zip");
    let mut writer = ::zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    writer.add_directory("private/", ::zip::write::SimpleFileOptions::default()).unwrap();
    let options = ::zip::write::SimpleFileOptions::default()
        .with_deprecated_encryption(b"fixture-password-never-provided-to-core")
        .unwrap();
    writer.start_file("private/secret.txt", options).unwrap();
    writer.write_all(b"ENCRYPTED_ENTRY_BODY_MUST_NOT_BE_SENT").unwrap();
    writer.finish().unwrap();
    let mut check = ::zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    assert!(check.by_index(1).is_err(), "fixture content really requires a password");
    drop(check);
    let started = services(&runtime)
        .agent_required()
        .start(
            CommandInvocationId::new(),
            start_request(session, model, &path, "List archive metadata only"),
        )
        .await
        .unwrap();
    finish(&runtime, &started).await;
    let text = request_text(&provider);
    assert!(text.contains("private/") && text.contains("[目录]"), "{text}");
    assert!(text.contains("private/secret.txt") && text.contains("[加密]"), "{text}");
    assert!(!text.contains("ENCRYPTED_ENTRY_BODY_MUST_NOT_BE_SENT"));
    assert!(!path.parent().unwrap().join("private").exists(), "nothing was extracted");
}
