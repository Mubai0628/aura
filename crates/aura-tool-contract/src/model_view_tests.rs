use super::*;

/// A write's result as stored, from the shape of a real DeepSeek run.
const STORED: &str = r#"{"schema_version":1,"tool_call_id":"tcl_01M5H42MBWBKYQRHR1GJXWXPS4","definition_ref":{"tool_id":"tol_01M2DG020980080021AN942003","tool_version":{"semantic_version":null,"definition_revision":"7"},"registry_snapshot_ref":{"tool_registry_snapshot_id":"trs_01M5H3T29P3WJ882BA3FWZCQH8","registry_revision":"1","content_hash":{"algorithm":"sha256","canonicalization_version":"rfc8785_jcs_v1","hex":"f07d9e395ff46a2e73ae101add9721f79399ae0ee8569fcdd3450622fc452319"}}},"outcome":{"type":"succeeded","payload":{"type":"structured_json","value":{"affected_paths":["game.html"],"catalog_refresh_pending":false,"deferred_mutation_id":null,"disposition":"applied","external":null,"requires_reread":false,"workspace_revision":"1446"}}},"side_effect_state":"fully_applied","warnings":[],"artifacts":[{"artifact_id":"art_01M5H45NQCEVZSSC7FT5BEFER4","slot":"result","media_type":"application/vnd.aura.model-tool-outcome+json","byte_length":257,"content_hash":{"algorithm":"sha256","hex":"cf21c794b643674ca33c7697f3e40bc1ec668c07c7d64088792de10c5a9e8e71"}}],"completed_at":"2026-10-22T06:18:29.9526801Z"}"#;

#[test]
fn the_model_reads_the_outcome_and_no_hash_it_could_mistake_for_a_files() {
    let envelope: ModelVisibleToolResultEnvelopeV1 =
        serde_json::from_str(STORED).expect("a stored envelope");
    let text = envelope.model_text();
    for noise in
        ["f07d9e395ff4", "cf21c794b643", "tcl_01M5H42", "registry_snapshot_ref", "completed_at"]
    {
        assert!(!text.contains(noise), "{noise} is not shown to the model: {text}");
    }
    let view = envelope.model_view();
    assert_eq!(view["outcome"]["payload"]["value"]["affected_paths"][0], "game.html");
    assert_eq!(view["side_effect_state"], "fully_applied");
    assert_eq!(view["artifacts"][0]["artifact_id"], "art_01M5H45NQCEVZSSC7FT5BEFER4");
    assert!(view.get("warnings").is_none(), "no empty list");
    assert!(text.len() * 2 < STORED.len(), "less than half the stored envelope: {}", text.len());
}

/// The stored write with `outcome`, `side_effect_state` and `warnings` in place of its own.
fn stored(
    outcome: serde_json::Value,
    side_effect_state: &str,
    warnings: &[&str],
) -> ModelVisibleToolResultEnvelopeV1 {
    let mut value: serde_json::Value = serde_json::from_str(STORED).expect("a stored envelope");
    value["outcome"] = outcome;
    value["side_effect_state"] = serde_json::json!(side_effect_state);
    value["warnings"] = serde_json::json!(warnings);
    serde_json::from_value(value).expect("an envelope")
}

/// A call the user refused, or one a stopped turn never started, read as a reason enum and
/// "error": null.
#[test]
fn a_call_that_was_not_run_says_why_what_changed_and_what_to_do_next() {
    let denied = stored(
        serde_json::json!({"type": "not_executed", "reason": "approval_denied", "error": null}),
        "not_started",
        &[],
    );
    let view = denied.model_view();
    assert_eq!(
        view["message"],
        "This call was not run: the user denied it when asked for approval. Nothing was changed. Do not repeat it unchanged; continue without it, or ask the user what they want instead."
    );
    let told = stored(
        serde_json::json!({
            "type": "not_executed",
            "reason": "dependency_not_executed",
            "error": {"code": "tool.dispatch_refused", "category": "conflict", "message": "x", "retryable": false}
        }),
        "not_started",
        &[],
    );
    assert!(told.model_view().get("message").is_none(), "a notice of its own is enough");
}

#[test]
fn a_stopped_call_says_what_it_may_have_changed() {
    let stopped = stored(
        serde_json::json!({"type": "cancelled", "reason": "timeout"}),
        "unassessed",
        &["execution.isolation_reduced", "artifact.unavailable"],
    );
    let view = stopped.model_view();
    assert_eq!(
        view["message"],
        "This call was stopped before it finished: it ran past its time limit. Aura did not check what it changed; check the target before trying again."
    );
    assert_eq!(
        view["warnings"],
        serde_json::json!([
            "It ran with the user's own rights, not in a sandbox.",
            "artifact.unavailable"
        ])
    );
    let plain = stored(
        serde_json::json!({"type": "cancelled", "reason": "user_requested"}),
        "no_effect",
        &[],
    );
    assert_eq!(
        plain.model_view()["message"],
        "This call was stopped before it finished. Nothing was changed. Call it again if it is still needed."
    );
}

/// A call whose turn was stopped in the moment after it was marked started, before anything of it
/// was sent, did not run; one the turn cut while it ran says what it may have changed.
#[test]
fn a_call_its_stopped_turn_never_started_reads_as_not_run() {
    let ended = stored(
        serde_json::json!({"type": "cancelled", "reason": "turn_interrupted"}),
        "no_effect",
        &[],
    );
    assert_eq!(
        ended.model_view()["message"],
        "This call was not run: the turn was stopped before it started. Nothing was changed. Call it again if it is still needed."
    );
    let cut = stored(
        serde_json::json!({"type": "cancelled", "reason": "turn_interrupted"}),
        "partially_applied",
        &[],
    );
    assert_eq!(
        cut.model_view()["message"],
        "This call was stopped before it finished: the turn it belonged to was interrupted. Part of its changes may have been made; check the target before trying again."
    );
}
