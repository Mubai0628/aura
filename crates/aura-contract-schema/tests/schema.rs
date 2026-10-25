use aura_contract_schema::{canonical_fixture, canonical_schema_value};
use aura_projection_contract::ProjectionKey;
use serde_json::{Value, json};

#[test]
fn canonical_schema_is_deterministic() {
    let first = canonical_schema_value().expect("schema should generate");
    let second = canonical_schema_value().expect("schema should generate twice");
    assert_eq!(first, second);
}

#[test]
fn canonical_fixture_round_trips_without_changing_windows_paths() {
    let fixture = canonical_fixture().expect("fixture should build");
    let encoded = serde_json::to_value(&fixture).expect("fixture should serialize");
    let decoded: aura_contract_schema::CanonicalContractFixture =
        serde_json::from_value(encoded.clone()).expect("fixture should deserialize");

    assert_eq!(decoded, fixture);
    assert_eq!(encoded["windows_path"], json!(r"C:\Users\测试用户\Aura Workspace\src\main.rs"));
    assert_eq!(
        encoded["models"]["execution_snapshot"]["id"],
        encoded["ids"]["model_execution_snapshot_id"]
    );
    assert_eq!(
        encoded["models"]["stream_event"]["event"]["provider_call_id"],
        json!("Call_Contract_ExactCase")
    );
}

#[test]
fn canonical_fixture_rejects_unknown_fields() {
    let fixture = canonical_fixture().expect("fixture should build");
    let mut encoded = serde_json::to_value(fixture).expect("fixture should serialize");
    encoded
        .as_object_mut()
        .expect("fixture is an object")
        .insert("unknown".to_owned(), Value::Bool(true));

    let error = serde_json::from_value::<aura_contract_schema::CanonicalContractFixture>(encoded)
        .expect_err("unknown fields must be rejected");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn canonical_fixture_rejects_unknown_fields_inside_p05_contracts() {
    let fixture = canonical_fixture().expect("fixture should build");
    let mut encoded = serde_json::to_value(fixture).expect("fixture should serialize");
    let provider_id = encoded["ids"]["provider_id"].as_str().expect("provider ID").to_owned();
    encoded["models"]["registry"]["providers"][&provider_id]
        .as_object_mut()
        .expect("provider config is an object")
        .insert("unknown".to_owned(), Value::Bool(true));

    let error = serde_json::from_value::<aura_contract_schema::CanonicalContractFixture>(encoded)
        .expect_err("unknown P05 fields must be rejected");
    assert!(error.to_string().contains("unknown field"));
}

#[test]
fn canonical_fixture_covers_frozen_p05_shared_contracts_without_secrets() {
    let fixture = canonical_fixture().expect("fixture should build");
    let encoded = serde_json::to_value(&fixture).expect("fixture should serialize");

    assert!(encoded["ids"]["tool_id"].as_str().expect("ToolId").starts_with("tol_"));
    assert_eq!(
        encoded["models"]["tool_versions"][0]["semantic_version"],
        "1.2.3-alpha.1+contract.5"
    );
    assert_eq!(encoded["models"]["tool_versions"][1]["semantic_version"], Value::Null);

    let endpoint_id = encoded["ids"]["endpoint_id"].as_str().expect("endpoint ID");
    let credential_id = encoded["ids"]["credential_id"].as_str().expect("CredentialId");
    let endpoint = &encoded["models"]["registry"]["endpoints"][endpoint_id];
    assert!(endpoint.get("credential_ref").is_none());

    let configure_request = &encoded["models"]["configure_credential_request"];
    assert_eq!(configure_request["expected_models_revision"], "3");
    assert_eq!(configure_request["credential_slot"], "api-key");
    assert_eq!(configure_request["credential_id"], credential_id);
    assert_eq!(configure_request["expected_row_revision"], "7");

    let credential_status = &encoded["models"]["credential_mutation_result"]["credential_status"];
    let credential_revision = &encoded["models"]["credential_revision"];
    assert_eq!(credential_status["credential_id"], credential_id);
    assert_eq!(credential_status["state"], "configured");
    assert_eq!(&credential_status["active_credential_revision"], credential_revision);
    assert_eq!(&credential_status["last_committed_credential_revision"], credential_revision);
    assert_eq!(credential_status["row_revision"], "7");
    assert_eq!(credential_status["error_code"], Value::Null);
    assert!(credential_status.get("credential_ref").is_none());
    assert!(credential_status.get("credential_binding_fingerprint").is_none());

    assert_eq!(encoded["models"]["credential_mutation_result"]["models_revision"], "3");
    assert!(encoded["models"]["credential_mutation_result"].get("revision").is_none());

    let created = &encoded["models"]["endpoint_create_with_credential_result"];
    assert_eq!(created.as_object().expect("create-with-credential result").len(), 2);
    assert!(created.get("credential_status").is_none());
    assert_eq!(created["models_revision"], "3");
    let authentication = &created["endpoint_projection"]["authentication"];
    assert_eq!(authentication["auth_kind"], "secret");
    assert_eq!(authentication["dormant_credential"], Value::Null);
    assert_eq!(authentication["active_credential"]["state"], "configured");
    assert_eq!(
        authentication["active_credential"]["active_credential_revision"],
        authentication["active_credential"]["last_committed_credential_revision"]
    );

    let deletion = &encoded["models"]["deletion_mutation_result"];
    assert_eq!(deletion["changed"], true);
    assert_eq!(deletion["value"]["credential_cleanup_pending"], true);
    assert_eq!(deletion["value"]["deleted"]["deleted_endpoint_ids"][0], endpoint_id);

    let routes = encoded["runtime"]["projection_keys"].as_array().expect("projection keys");
    assert_eq!(routes.len(), 10);
    for route in routes {
        let key: ProjectionKey =
            serde_json::from_value(route.clone()).expect("valid projection route");
        key.validate().expect("registered kind/scope");
    }

    let evidence_headers = encoded["models"]["provider_request_evidence"]["headers"]
        .as_array()
        .expect("request evidence headers");
    let authorization = evidence_headers
        .iter()
        .find(|header| header["name"] == "authorization")
        .expect("redacted authorization metadata");
    assert_eq!(authorization["non_secret_value"], Value::Null);
    let encoded_models = encoded["models"].to_string().to_ascii_lowercase();
    for forbidden in ["\"secret\":", "\"api_key\":", "\"x-api-key\":\""] {
        assert!(!encoded_models.contains(forbidden), "forbidden secret field: {forbidden}");
    }
}
