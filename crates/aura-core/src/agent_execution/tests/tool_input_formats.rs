use super::*;

const ORIGINAL: &str = "one\ntwo\n";
const DIFF: &str = "--- a/notes.txt\n+++ b/notes.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+TWO\n";

/// Exercise provider normalization, admission, broker storage and the correction fed back to the model.
#[tokio::test]
async fn malformed_file_hashes_name_the_field_before_approval_or_execution() {
    let f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["fs.patch", "fs.write"],
        |root| {
            std::fs::write(root.join("notes.txt"), ORIGINAL).expect("fixture file");
        },
    )
    .await;
    let cases = [
        ("fs.patch", "a".repeat(60), "/expected_hash/hex"),
        ("fs.patch", "b".repeat(82), "/expected_hash/hex"),
        ("fs.patch", "c".repeat(49), "/expected_hash/hex"),
        ("fs.write", "A".repeat(64), "/expected/hex"),
        ("fs.write", "z".repeat(64), "/expected/hash/hex"),
    ];
    let calls: Vec<_> = cases
        .iter()
        .map(|(name, hex, field)| {
            let hash = json!({"algorithm":"blake3","hex":hex});
            let args = if *name == "fs.patch" {
                json!({"path":"notes.txt","unified_diff":DIFF,"expected_hash":hash})
            } else {
                let expected = if *field == "/expected/hash/hex" {
                    json!({"expected_kind":"file_hash","hash":hash})
                } else {
                    hash
                };
                json!({"path":"notes.txt","expected":expected,"text":"replacement"})
            };
            (*name, args)
        })
        .collect();
    let batch = f.admit(&calls);
    let ids: Vec<_> = batch.batch.calls.iter().map(|c| c.tool_call_id).collect();
    f.execute(batch).await;

    let store = f.service().broker.lock().expect("store");
    let db = store.database.connection();
    for (id, (name, hex, field)) in ids.into_iter().zip(cases) {
        let call = aura_persistence_v2::tool::execution::call(db, id).expect("recorded call");
        let result = call.result.expect("paired tool result");
        let ToolResultOutcomeV1::NotExecuted { error: Some(error), .. } = &result.outcome else {
            panic!("{name} {field}: expected an argument refusal, got {:?}", result.outcome);
        };
        assert!(call.attempt_id.is_none(), "invalid input started an execution attempt");
        let bound: bool = db
            .query_row(
                "SELECT operation_id IS NOT NULL FROM tool_calls WHERE id=?1",
                [id.to_string()],
                |row| row.get(0),
            )
            .expect("stored operation binding");
        assert!(!bound, "invalid input reached operation construction");
        assert_eq!(result.side_effect_state, SideEffectStateV1::NotStarted);
        assert_eq!(error.code.as_str(), "tool.schema.arguments_invalid");
        let bytes = aura_persistence_core::artifact::read(
            db,
            result.model_visible_result_ref.expect("model result"),
            &id.to_string(),
        )
        .expect("stored model response");
        let text = String::from_utf8(bytes).expect("model-readable UTF-8");
        for expected in [field, "64", "小写", "fs.read", "logical_hash"] {
            assert!(text.contains(expected), "{name} {field}: missing {expected} in {text}");
        }
        assert!(!text.contains(&hex), "the correction must not echo the submitted value");
    }
    assert_eq!(std::fs::read_to_string(f.root.join("notes.txt")).expect("original file"), ORIGINAL);
}

/// Admission, the model receipt and the desktop all see the same mixed batch.
#[tokio::test]
async fn shell_model_budget_refusal_and_neighbor_are_visible_to_both_consumers() {
    let f = Fixture::with_exposure_setup(
        PermissionMode::WorkspaceReadWrite,
        &["shell.powershell", "fs.read"],
        |root| {
            std::fs::write(root.join("notes.txt"), "neighbor").expect("fixture file");
        },
    )
    .await;
    let batch = f.admit(&[
        (
            "shell.powershell",
            json!({"script":"Write-Output 42","max_output_bytes":6500,"max_output_tokens":7000}),
        ),
        ("fs.read", json!({"path":"notes.txt"})),
    ]);
    let ids: Vec<_> = batch.batch.calls.iter().map(|c| c.tool_call_id).collect();
    f.execute(batch).await;
    let store = f.service().broker.lock().expect("store");
    let db = store.database.connection();
    let bad = aura_persistence_v2::tool::execution::call(db, ids[0]).expect("refused call");
    assert!(bad.attempt_id.is_none());
    let receipt = bad.result.expect("terminal refusal");
    assert!(matches!(receipt.outcome, ToolResultOutcomeV1::NotExecuted { .. }));
    assert_eq!(receipt.side_effect_state, SideEffectStateV1::NotStarted);
    let data = aura_persistence_core::artifact::read(
        db,
        receipt.model_visible_result_ref.expect("model receipt"),
        &ids[0].to_string(),
    )
    .expect("model-visible refusal");
    let text = String::from_utf8(data).expect("UTF-8");
    assert!(
        text.contains("/max_output_tokens") && text.contains("max_output_bytes` (bytes)"),
        "{text}"
    );
    let page = aura_persistence_v2::tool::activity::page(
        db,
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 2,
            call_ids: Some(ids),
        },
    )
    .expect("mixed page");
    assert!(page.unreadable_call_ids.is_empty());
    assert_eq!(page.calls.len(), 2);
    assert!(!page.calls[0].shell_query_available);
    assert!(page.calls[0].display.details.iter().any(|s| s.contains("/max_output_tokens")));
    assert_eq!(page.calls[1].summary.display_name, "fs.read");
    assert!(matches!(
        page.calls[1].summary.outcome,
        Some(aura_tool_projection::ToolResultOutcomeSummaryView::Succeeded)
    ));
}
