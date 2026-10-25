use aura_tool_eval::owner::OwnerSource;
use serde_json::json;

#[test]
fn owner_metadata_is_read_only_and_a_changed_tuple_requires_a_new_estimate() {
    let root = tempfile::tempdir().expect("isolated owner metadata");
    let models = root.path().join("models.json");
    let database = root.path().join("state.db");
    let value = json!({"models":{"selected":{"enabled":true,"provider_model_name":"deepseek-owner-model","endpoint_id":"owner-endpoint","request_defaults":{"max_output_tokens":8192},"context":{"runtime_effective_window_tokens":256000,"historical_snapshot_max_tokens":1024}}},"endpoints":{"owner-endpoint":{"base_url":"https://provider.example/v1","api_format":"open_ai_responses","auth_scheme":"open_ai_bearer_api_key"}}});
    let before = serde_json::to_vec(&value).expect("fixture bytes");
    std::fs::write(&models, &before).expect("config fixture");
    let db = rusqlite::Connection::open(&database).expect("metadata fixture");
    db.execute_batch("CREATE TABLE credential_records(endpoint_id TEXT,credential_slot TEXT,state TEXT,credential_ref TEXT); INSERT INTO credential_records VALUES('owner-endpoint','api-key','configured','wincred://aura/eval-fixture/api-key');").expect("only reference metadata, no secret column");
    drop(db);
    let source = OwnerSource {
        models_path: models.clone(),
        database_path: database.clone(),
        model_profile_id: "selected".into(),
    };
    let target = source.read().expect("read metadata, never WinCred");
    assert_eq!(target.model, "deepseek-owner-model");
    assert_eq!(target.request_url, "https://provider.example/v1/responses");
    assert_eq!(target.credential_ref.to_string(), "wincred://aura/eval-fixture/api-key");
    assert_eq!(std::fs::read(&models).expect("config unchanged"), before);
    let mut changed = value;
    changed["models"]["selected"]["provider_model_name"] = json!("deepseek-another-model");
    std::fs::write(&models, serde_json::to_vec(&changed).expect("changed fixture"))
        .expect("simulate owner changing metadata");
    assert!(
        target.verify_current().is_err(),
        "a prepared estimate cannot silently use another model"
    );
    std::fs::write(&models, before).expect("restore fixture");
    let db =
        rusqlite::Connection::open(database).expect("simulate configured credential revocation");
    db.execute("UPDATE credential_records SET state='cleared'", []).expect("cleared metadata");
    assert!(target.verify_current().is_err(), "a cleared credential reference is not offered");
}
