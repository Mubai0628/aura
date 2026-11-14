use aura_tool_eval::owner::OwnerSource;
use serde_json::json;

#[test]
fn r2_owner_requests_use_responses_and_freeze_every_request_setting() {
    let root = tempfile::tempdir().expect("isolated owner metadata");
    let models = root.path().join("models.json");
    let database = root.path().join("state.db");
    let mut config = json!({
        "models":{"selected":{"enabled":true,"provider_model_name":"deepseek-flash","endpoint_id":"endpoint",
          "request_defaults":{"max_output_tokens":8192,"temperature":0.6,"top_p":0.98,"chat_output_token_field":null,"chat_instruction_role":null},
          "context":{"runtime_effective_window_tokens":256000,"historical_snapshot_max_tokens":1024}}},
        "endpoints":{"endpoint":{"base_url":"https://provider.example","api_format":"open_ai_responses","auth_scheme":"open_ai_bearer_api_key","max_parallel_model_requests":2}}
    });
    std::fs::write(&models, serde_json::to_vec(&config).expect("metadata")).expect("source");
    let db = rusqlite::Connection::open(&database).expect("reference metadata");
    db.execute_batch("CREATE TABLE credential_records(endpoint_id TEXT,credential_slot TEXT,state TEXT,credential_ref TEXT); INSERT INTO credential_records VALUES('endpoint','api-key','configured','wincred://aura/eval-fixture/api-key');").expect("no secret column");
    drop(db);
    let source = OwnerSource {
        models_path: models.clone(),
        database_path: database,
        model_profile_id: "selected".into(),
    };
    let target = source.read().expect("read only");
    assert_eq!(target.request_url, "https://provider.example/responses");
    let serialized = serde_json::to_value(&target).expect("non-secret target");
    assert_eq!(serialized["settings"]["request_defaults"]["max_output_tokens"], 8192);
    assert_eq!(serialized["settings"]["context_window_tokens"], 256000);
    for (pointer, value) in [
        ("/models/selected/request_defaults/max_output_tokens", json!(4096)),
        ("/models/selected/request_defaults/temperature", json!(0.7)),
        ("/models/selected/request_defaults/top_p", json!(0.99)),
        ("/models/selected/context/runtime_effective_window_tokens", json!(128000)),
        ("/endpoints/endpoint/api_format", json!("open_ai_chat_completions")),
        ("/endpoints/endpoint/chat_output_token_field", json!("max_completion_tokens")),
        ("/endpoints/endpoint/chat_instruction_role", json!("developer")),
    ] {
        let previous = config.clone();
        // New optional endpoint fields are explicitly present so pointer mutation is exact.
        if pointer.ends_with("chat_output_token_field") && pointer.contains("endpoints") {
            config["endpoints"]["endpoint"]["chat_output_token_field"] = value;
        } else if pointer.ends_with("chat_instruction_role") && pointer.contains("endpoints") {
            config["endpoints"]["endpoint"]["chat_instruction_role"] = value;
        } else {
            *config.pointer_mut(pointer).expect("fixture field") = value;
        }
        std::fs::write(&models, serde_json::to_vec(&config).expect("changed metadata"))
            .expect("source change");
        assert!(
            target.verify_current().is_err(),
            "changed request setting {pointer} must require re-estimation"
        );
        config = previous;
    }
}

#[test]
fn r2_probe_keeps_the_product_output_budget_for_reasoning() {
    for case in aura_tool_eval::probe::cases("deepseek-flash") {
        let limit =
            case.request.get("max_output_tokens").or_else(|| case.request.get("max_tokens"));
        assert_eq!(limit, Some(&json!(8192)), "probe must not truncate reasoning at 256 tokens");
    }
}
