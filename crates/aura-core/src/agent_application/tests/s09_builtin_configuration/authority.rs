use super::*;
use aura_domain_common::jcs::{canonical_json_jcs, canonical_sha256};
use aura_tool_domain::{ToolDefinitionRecordV1, ToolInstanceRecordV1};

fn canonical<T: serde::Serialize>(value: &T) -> String {
    String::from_utf8(canonical_json_jcs(value).expect("canonical fixture JSON"))
        .expect("canonical JSON is UTF-8")
}

pub(super) fn set_instance(s: &crate::RuntimeServices, enabled: bool, timeout: Option<u64>) {
    let entry = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP");
    let mut instance = s
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("authority")
        .instances
        .into_iter()
        .find(|i| i.tool_instance_id == entry.instance.tool_instance_id)
        .expect("HTTP owner");
    instance.enabled = enabled;
    instance.overrides.timeout_ms = timeout;
    instance.entity_revision = Revision::new(instance.entity_revision.value() + 1);
    instance.updated_at = UtcTimestamp::now();
    let mut db = rusqlite::Connection::open(s.persistence.database_path())
        .expect("isolated configuration DB");
    let tx = db.transaction().expect("fixture configuration transaction");
    tx.execute("UPDATE tool_instances SET enabled=?1,entity_revision=?2,record_json=?3,record_hash_json=?4,updated_at=?5 WHERE tool_instance_id=?6",
        rusqlite::params![enabled,i64::try_from(instance.entity_revision.value()).expect("revision"),canonical(&instance),canonical(&canonical_sha256(&instance).expect("instance hash")),instance.updated_at.to_string(),instance.tool_instance_id.to_string()]).expect("persist saved instance facts");
    tx.execute("UPDATE tool_registry_authority_state SET registry_revision=registry_revision+1,updated_at=?1",[instance.updated_at.to_string()]).expect("configuration cache revision");
    tx.commit().expect("commit isolated configuration");
    s.persistence.tool_authority_snapshot(None, None, None).expect("valid saved authority");
}

pub(super) fn seed_disabled_previous(s: &crate::RuntimeServices) {
    let old: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/tests/s09/http-request-v1.json"
    )))
    .expect("published S07 HTTP v1 fixture");
    let definition: ToolDefinitionRecordV1 =
        serde_json::from_value(old["definition"].clone()).expect("historical definition");
    let mut instance: ToolInstanceRecordV1 =
        serde_json::from_value(old["instance"].clone()).expect("historical instance");
    instance.enabled = false;
    instance.entity_revision = Revision::new(3);
    instance.updated_at = UtcTimestamp::now();
    let current = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .instance
        .tool_instance_id;
    let mut db =
        rusqlite::Connection::open(s.persistence.database_path()).expect("isolated upgrade DB");
    let tx = db.transaction().expect("historical seed transaction");
    tx.execute("INSERT INTO tool_definitions(tool_id,tool_version_json,tool_package_id,tool_key,canonical_name,definition_revision,definition_fingerprint_json,entity_revision,record_json,record_hash_json,retired_at) VALUES(?1,?2,NULL,NULL,?3,1,?4,1,?5,?6,NULL)",
        rusqlite::params![definition.tool_id.to_string(),canonical(&definition.tool_version),definition.canonical_name.as_str(),canonical(&definition.definition_fingerprint),canonical(&definition),canonical(&canonical_sha256(&definition).expect("definition hash"))]).expect("historical published definition");
    tx.execute("INSERT INTO tool_instances(tool_instance_id,tool_id,tool_version_json,source_kind,enabled,runtime_health,entity_revision,record_json,record_hash_json,created_at,updated_at) VALUES(?1,?2,?3,'built_in',0,'healthy',3,?4,?5,?6,?7)",
        rusqlite::params![instance.tool_instance_id.to_string(),instance.tool_id.to_string(),canonical(&instance.tool_version),canonical(&instance),canonical(&canonical_sha256(&instance).expect("instance hash")),instance.created_at.to_string(),instance.updated_at.to_string()]).expect("disabled historical owner");
    tx.execute("DELETE FROM tool_instances WHERE tool_instance_id=?1", [current.to_string()])
        .expect("remove only this fixture's empty current owner");
    tx.commit().expect("historical configuration seed");
}

pub(super) async fn expose_http(s: &crate::RuntimeServices) {
    let view = s.agent_required().tool_settings().expect("settings");
    let http = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .definition
        .tool_id;
    let mut fixed = view.fixed_tool_ids.clone();
    if !fixed.contains(&http) {
        // The settings contract takes a sorted list; some recommended
        // tools (memory.*) sort after HTTP, so appending alone is not enough.
        fixed.push(http);
        fixed.sort();
    }
    s.agent_required()
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: view.revision,
                fixed_tool_ids: fixed,
                max_dynamic_tool_schemas: view.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: view.max_concurrent_tool_calls,
                default_timeout_ms: view.default_timeout_ms,
                max_retries: view.max_retries,
                use_recommended_fixed_tool_ids: false,
                network_proxy: None,
                web_service: None,
            },
        )
        .await
        .expect("fixed HTTP visibility is a user's explicit setting");
}

pub(super) async fn run_text(
    host: &crate::r03_tests::Fixture,
    runtime: &crate::RuntimeState,
    model: ModelProfileId,
    provider: &Provider,
    label: &str,
) -> Vec<Value> {
    let cursor = provider.requests.lock().expect("request cursor").len();
    let root = host.root.path().join(label);
    std::fs::create_dir(&root).expect("isolated workspace");
    let created = crate::r03_tests::success(
        runtime,
        "workspace.create",
        json!({"selected_root":root,"display_name":label}),
    )
    .await;
    let workspace: WorkspaceId =
        serde_json::from_value(created["workspace"]["workspace_id"].clone()).expect("workspace id");
    let session = create_session(runtime, workspace, PermissionMode::FullAccess).await;
    let start: AgentRunProjection = serde_json::from_value(
        crate::r03_tests::success(
            runtime,
            "agent.start",
            json!(AgentStartRequest {
                session_id: session,
                model_profile_id: model,
                text: "Give a short answer without tools.".into(),
                mode: AgentTaskMode::Normal,
                goal_budget: None,
                explicit_backend_id: None,
                skill_ids: Vec::new(),
                reasoning_effort: None,
                attachments: Vec::new(),
            }),
        )
        .await,
    )
    .expect("Agent start");
    let finished = wait_for_terminal(runtime, session, start.run.agent_run_id).await;
    let requests = provider.requests.lock().expect("actual provider requests")[cursor..].to_vec();
    assert!(!requests.is_empty(), "the real model transport was reached: {finished:?}");
    requests
}
