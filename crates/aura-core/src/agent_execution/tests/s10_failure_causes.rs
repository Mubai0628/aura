//! A file tool call that fails before changing anything tells
//! the model why and what to do next. A bare "the execution did not succeed; do not retry an
//! unknown effect" contradicts the no-effect fact Core already recorded. Evaluation samples hit two
//! such calls: a unified diff whose hunk header did not match its lines (the model then gave up on
//! fs.patch for the rest of the task), and fs.read on a directory.
use super::*;

const NOTES: &str = "one\ntwo\nthree\n";

async fn workspace_with_notes(mode: PermissionMode, tools: &[&str]) -> Fixture {
    Fixture::with_exposure_setup(mode, tools, |root| {
        std::fs::write(root.join("notes.txt"), NOTES).expect("Agent test setup");
        std::fs::create_dir(root.join("docs")).expect("Agent test setup");
    })
    .await
}

fn hash(text: &str) -> serde_json::Value {
    serde_json::to_value(aura_domain_common::ContentHash::blake3(text.as_bytes()))
        .expect("Agent test setup")
}

async fn run(f: &Fixture, name: &str, arguments: serde_json::Value) -> ToolResultV1 {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let store = f.service().broker.lock().expect("persisted result");
    aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("persisted result")
        .result
        .expect("persisted result")
}

/// The refusal the model sees, after checking that the call really changed nothing.
fn rejection(result: ToolResultV1) -> aura_domain_common::ErrorEnvelope {
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    let ToolResultOutcomeV1::OperationRejected { error } = result.outcome else {
        panic!("a refusal naming its cause is required: {:?}", result.outcome)
    };
    error
}

fn notes(f: &Fixture) -> String {
    std::fs::read_to_string(f.root.join("notes.txt")).expect("notes.txt")
}

#[tokio::test]
async fn a_patch_with_recounted_lines_still_reports_a_real_context_mismatch() {
    let f = workspace_with_notes(PermissionMode::WorkspaceReadWrite, &["fs.patch"]).await;
    // The header promises seven lines and the hunk has three, as in an evaluation run.
    let diff =
        "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,7 +1,7 @@\n wrong context\n-two\n+TWO\n three\n";
    let result = run(
        &f,
        "fs.patch",
        json!({"path":"notes.txt","expected_hash":hash(NOTES),"unified_diff":diff}),
    )
    .await;
    let error = rejection(result);
    assert_eq!(error.code.as_str(), "fs.invalid_patch");
    assert_eq!(error.category, ErrorCategory::Validation);
    // The message names the line the file does not have, and what it has there.
    assert!(error.message.contains("hunk #1"), "{}", error.message);
    assert!(error.message.contains("«wrong context»"), "{}", error.message);
    assert!(error.message.contains("Nothing was changed"), "{}", error.message);
    assert_eq!(notes(&f), NOTES);
}

#[tokio::test]
async fn a_write_against_a_stale_hash_asks_for_a_fresh_read_and_changes_nothing() {
    let f = workspace_with_notes(PermissionMode::WorkspaceReadWrite, &["fs.write"]).await;
    let stale = json!({"expected_kind":"file_hash","hash":hash("an older version\n")});
    let result =
        run(&f, "fs.write", json!({"path":"notes.txt","text":"replaced\n","expected":stale})).await;
    let error = rejection(result);
    assert_eq!(error.code.as_str(), "fs.expected_state_conflict");
    assert_eq!(error.category, ErrorCategory::Conflict);
    assert!(error.message.contains("Read the file again"), "{}", error.message);
    // A mistyped hash looks the same as a changed file, so the message names both.
    assert!(error.message.contains("copied incorrectly"), "{}", error.message);
    assert!(error.message.contains("Nothing was changed"), "{}", error.message);
    assert_eq!(notes(&f), NOTES);
}

#[tokio::test]
async fn reading_a_directory_says_it_is_a_directory_and_points_to_fs_list() {
    let f = workspace_with_notes(PermissionMode::ReadOnly, &["fs.read"]).await;
    let error = rejection(run(&f, "fs.read", json!({"path":"docs"})).await);
    assert_eq!(error.code.as_str(), "fs.wrong_kind");
    assert_eq!(error.category, ErrorCategory::Validation);
    assert!(error.message.contains("is a directory"), "{}", error.message);
    assert!(error.message.contains("fs.list"), "{}", error.message);
}

#[tokio::test]
async fn reading_a_file_another_program_holds_says_it_is_in_use() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = workspace_with_notes(PermissionMode::ReadOnly, &["fs.read"]).await;
    // Another program holds the file without sharing it. Windows reports a sharing violation,
    // which the standard library leaves uncategorized.
    let held = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.root.join("notes.txt"))
        .expect("exclusive handle");
    let result = run(&f, "fs.read", json!({"path":"notes.txt"})).await;
    drop(held);
    let error = rejection(result);
    assert_eq!(error.code.as_str(), "fs.file_in_use");
    assert_eq!(error.category, ErrorCategory::Conflict);
    assert!(error.message.contains("Try again"), "{}", error.message);
}
