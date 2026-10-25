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
