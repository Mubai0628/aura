//! Real admission, execution and persisted results for the approved input changes.
//!
//! Each scenario uses its own fixture: the fixture admits one model step's output once.
use super::*;

const NOTES: &str = "one\ntwo\nthree\n";

async fn fixture(tools: &[&str]) -> Fixture {
    Fixture::with_exposure_setup(PermissionMode::WorkspaceReadWrite, tools, |root| {
        std::fs::write(root.join("notes.txt"), NOTES).expect("original file");
    })
    .await
}

async fn run(f: &Fixture, name: &str, arguments: serde_json::Value) -> ToolResultV1 {
    let batch = f.admit(&[(name, arguments)]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let store = f.service().broker.lock().expect("persisted result");
    aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("persisted call")
        .result
        .expect("persisted result")
}

fn notes(f: &Fixture) -> String {
    std::fs::read_to_string(f.root.join("notes.txt")).expect("notes.txt")
}

fn hash(text: &str) -> aura_domain_common::ContentHash {
    aura_domain_common::ContentHash::blake3(text.as_bytes())
}

#[tokio::test]
async fn a_bare_stale_write_hash_reaches_the_file_guard_and_changes_nothing() {
    let f = fixture(&["fs.write"]).await;
    let stale = run(
        &f,
        "fs.write",
        json!({"path":"notes.txt","text":"replacement\n","expected":hash("older\n")}),
    )
    .await;
    assert_eq!(stale.side_effect_state, SideEffectStateV1::NoEffect);
    let ToolResultOutcomeV1::OperationRejected { error } = stale.outcome else {
        panic!("a stale bare hash must reach the file guard: {:?}", stale.outcome);
    };
    assert_eq!(error.code.as_str(), "fs.expected_state_conflict");
    assert_eq!(notes(&f), NOTES);
}

#[tokio::test]
async fn a_bare_current_write_hash_writes_the_file() {
    let f = fixture(&["fs.write"]).await;
    let good = run(
        &f,
        "fs.write",
        json!({"path":"notes.txt","text":"replacement\n","expected":hash(NOTES)}),
    )
    .await;
    assert!(matches!(good.outcome, ToolResultOutcomeV1::Succeeded), "{:?}", good.outcome);
    assert_eq!(notes(&f), "replacement\n");
}

#[tokio::test]
async fn a_patch_with_wrong_counts_and_wrong_context_still_changes_nothing() {
    let f = fixture(&["fs.patch"]).await;
    let diff = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,7 +1,7 @@\n WRONG\n-two\n+TWO\n three\n";
    let result = run(
        &f,
        "fs.patch",
        json!({"path":"notes.txt","unified_diff":diff,"expected_hash":hash(NOTES)}),
    )
    .await;
    assert_eq!(result.side_effect_state, SideEffectStateV1::NoEffect);
    assert!(
        matches!(result.outcome, ToolResultOutcomeV1::OperationRejected { .. }),
        "{:?}",
        result.outcome
    );
    assert_eq!(notes(&f), NOTES);
}

#[tokio::test]
async fn a_patch_with_wrong_counts_and_matching_lines_applies() {
    let f = fixture(&["fs.patch"]).await;
    // The header promises seven lines and the hunk has three, as in an evaluation run.
    let diff = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,7 +1,7 @@\n one\n-two\n+TWO\n three\n";
    let result = run(
        &f,
        "fs.patch",
        json!({"path":"notes.txt","unified_diff":diff,"expected_hash":hash(NOTES)}),
    )
    .await;
    assert!(matches!(result.outcome, ToolResultOutcomeV1::Succeeded), "{:?}", result.outcome);
    assert_eq!(notes(&f), "one\nTWO\nthree\n");
    // The result names the file's hash after the patch,
    // the expected state of the next change.
    let store = f.service().broker.lock().expect("model-visible result");
    let visible = aura_persistence_core::artifact::read(
        store.database.connection(),
        result.model_visible_result_ref.expect("model-visible result"),
        &result.tool_call_id.to_string(),
    )
    .expect("model-visible result");
    let visible: serde_json::Value = serde_json::from_slice(&visible).expect("result json");
    let named = visible["outcome"]["payload"]["value"]["logical_hash"].clone();
    assert_eq!(named, serde_json::to_value(hash("one\nTWO\nthree\n")).expect("hash"), "{visible}");
}
