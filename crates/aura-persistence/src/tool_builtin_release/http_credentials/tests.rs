use super::*;
use crate::{PersistenceOptions, PersistenceStartup};
use aura_domain_common::{CredentialId, CredentialRevision, RuntimeInstanceId};
use aura_tool_contract::ToolVersion;

fn start(root: &tempfile::TempDir) -> PersistenceCoordinator {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        root.path().join("aura.db"),
        root.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("isolated configuration store")
    {
        PersistenceStartup::Ready(value) => value,
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

#[test]
fn s09_a_new_core_runtime_rebuilds_the_http_projection_without_revising_configuration() {
    use aura_projection_contract::{ProjectionKind, ProjectionScope};
    let root = tempfile::tempdir().expect("isolated restart store");
    let first = start(&root);
    first.materialize_http_credential_target(UtcTimestamp::now()).expect("initial HTTP owner");
    let tool_id = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .definition
        .tool_id;
    let revision = first.with_reader(read_registry_revision).expect("configuration revision");
    drop(first);
    let restarted = start(&root);
    restarted
        .materialize_http_credential_target(UtcTimestamp::now())
        .expect("restart initialization");
    restarted
        .rebuild_agent_builtin_configuration_projections()
        .expect("refresh runtime-owned views");
    assert!(
        restarted
            .latest_tool_projection(
                ProjectionKind::ToolDefinition,
                ProjectionScope::ToolDefinition { tool_id }
            )
            .expect("read current runtime projection")
            .is_some(),
        "a fresh Core runtime must provide the carried configuration to the real settings UI"
    );
    assert_eq!(
        restarted.with_reader(read_registry_revision).expect("unchanged registry revision"),
        revision,
        "rebuilding runtime projection must not modify configuration authority"
    );
    assert!(
        restarted
            .materialize_http_credential_target(UtcTimestamp::now())
            .expect("same runtime refresh")
            .is_none()
    );
    assert!(
        restarted
            .rebuild_agent_builtin_configuration_projections()
            .expect("same runtime views")
            .is_empty()
    );
}

fn legacy_entry() -> aura_tool_registry::agent_release::AgentBuiltinEntry {
    legacy_with_runtime(None)
}
fn legacy_with_runtime(
    runtime: Option<ToolRuntimeDescriptorV1>,
) -> aura_tool_registry::agent_release::AgentBuiltinEntry {
    let mut entry = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP tool")
        .clone();
    let mut d = entry.definition;
    if let Some(runtime) = runtime {
        d.runtime = runtime;
    }
    entry.definition = ToolDefinitionRecordV1::new(
        d.tool_id,
        ToolVersion::new(None, Revision::INITIAL).expect("old version"),
        d.tool_package_id,
        d.package_key,
        d.tool_key,
        d.canonical_name,
        d.display_name,
        d.model_description,
        d.model_summary,
        d.input_schema,
        d.strict,
        d.result_contract,
        d.runtime,
        d.capabilities,
        d.resource_claims,
        d.audience,
        d.source_kind,
        d.execution_semantics,
        d.runtime_file_closure_hash,
        d.entity_revision,
        d.created_at,
    )
    .expect("immutable old definition");
    let mut bytes = entry.instance.tool_instance_id.to_bytes();
    bytes[14] = 3;
    entry.instance.tool_instance_id = ToolInstanceId::from_bytes(bytes);
    entry.instance.tool_version = entry.definition.tool_version.clone();
    entry.instance.enabled = false;
    entry.instance.overrides.timeout_ms = Some(60_000);
    entry
}

#[test]
fn s09_a_builtin_upgrade_preserves_config_and_exact_credential_reference() {
    assert_preserved(aura_tool_domain::ToolCredentialStateV1::Configured);
}
#[test]
fn s09_a_builtin_upgrade_does_not_revive_a_revoked_credential() {
    assert_preserved(aura_tool_domain::ToolCredentialStateV1::Revoked);
}
fn assert_preserved(state: aura_tool_domain::ToolCredentialStateV1) {
    let root = tempfile::tempdir().expect("isolated store");
    let persistence = start(&root);
    let old = legacy_entry();
    let old_for_insert = old.clone();
    let owner = persistence.current_tool_principal().expect("profile owner");
    let expected_state = state;
    persistence.dispatch(move |db| {
        let tx = db.transaction()?;
        insert_definition(&tx, &old_for_insert.definition)?;
        insert_instance(&tx, &old_for_insert.instance)?;
        let config = ToolInstanceConfigurationRecordV1::new(old_for_insert.instance.tool_instance_id, Revision::new(3), CanonicalHashV1::sha256(b"config schema"), json!({"non_secret_option":"user choice"}), CanonicalHashV1::sha256(b"clean"), UtcTimestamp::now()).expect("user configuration");
        let (value, hash) = encode_record(&config)?;
        tx.execute("INSERT INTO tool_instance_configurations(tool_instance_id,configuration_revision,instance_config_schema_hash_json,configuration_json,configuration_hash_json,secret_scan_evidence_hash_json,record_json,record_hash_json,updated_at) VALUES(?1,3,?8,?2,?3,?4,?5,?6,?7)", params![config.tool_instance_id.to_string(),encode_json(&config.configuration)?,encode_json(&config.configuration_hash)?,encode_json(&config.secret_scan_evidence_hash)?,value,hash,config.updated_at.to_string(),encode_json(&config.instance_config_schema_hash)?])?;
        let binding = ToolInstanceCredentialBindingRecordV1::new(old_for_insert.instance.tool_instance_id, aura_tool_contract::CredentialSlotKey::new("api_key").expect("slot"), CredentialId::new(), CredentialRevision::new(Revision::INITIAL).expect("credential revision"), aura_tool_domain::ToolCredentialPlacementV1::HttpBearerAuthorization, Revision::new(4), UtcTimestamp::now()).expect("non-secret binding");
        let metadata = aura_tool_domain::ToolCredentialAuthorityRecordV1 {
            allowed_origins:vec!["https://example.test".into()], credential_id:binding.credential_id,
            credential_revision:binding.credential_revision,authenticated_owner:owner,
            scope:aura_tool_domain::ToolCredentialScopeV1::User,credential_kind:aura_tool_domain::ToolCredentialKindV1::ApiKey,
            state,
            placement_policy:aura_tool_domain::ToolCredentialPlacementPolicyV1 { allowed_placements:vec![binding.placement.clone()] },
            usage_policy:aura_tool_domain::ToolCredentialUsagePolicyV1::Explicit { tool_package_ids:vec![],tool_ids:vec![old_for_insert.definition.tool_id] },
            display_name:"Synthetic metadata without any secret".into(),row_revision:Revision::INITIAL,
            created_at:binding.updated_at,updated_at:binding.updated_at,
        };
        let (value,hash) = encode_record(&metadata)?;
        let stored_state = if state == aura_tool_domain::ToolCredentialStateV1::Configured {"configured"} else {"revoked"};
        tx.execute("INSERT INTO tool_credentials VALUES(?1,1,?2,?6,1,?3,?4,?5)",params![metadata.credential_id.to_string(),owner.to_string(),value,hash,metadata.updated_at.to_string(),stored_state])?;
        let (value, hash) = encode_record(&binding)?;
        tx.execute("INSERT INTO tool_instance_credential_bindings(tool_instance_id,credential_slot_key,credential_id,credential_revision,placement_json,binding_revision,binding_hash_json,record_json,record_hash_json,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)", params![binding.tool_instance_id.to_string(),binding.credential_slot_key.as_str(),binding.credential_id.to_string(),1,encode_json(&binding.placement)?,4,encode_json(&binding.binding_hash)?,value,hash,binding.updated_at.to_string()])?;
        tx.commit()?;
        Ok(())
    }).expect("seed old user-owned configuration without creating a secret");
    persistence
        .materialize_http_credential_target(UtcTimestamp::now())
        .expect("upgrade current HTTP owner");
    let current = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP tool")
        .instance
        .tool_instance_id;
    let authority =
        persistence.tool_authority_snapshot(None, None, None).expect("current authority");
    let new_instance =
        authority.instances.iter().find(|i| i.tool_instance_id == current).expect("new instance");
    assert!(!new_instance.enabled, "an upgrade must never re-enable the user's disabled tool");
    assert_eq!(new_instance.overrides, old.instance.overrides);
    let new_binding = authority
        .instance_credential_bindings
        .iter()
        .find(|b| b.tool_instance_id == current)
        .expect("binding carried forward");
    let old_binding = authority
        .instance_credential_bindings
        .iter()
        .find(|b| b.tool_instance_id == old.instance.tool_instance_id)
        .expect("old immutable binding retained");
    assert_eq!(new_binding.credential_id, old_binding.credential_id);
    assert_eq!(new_binding.credential_revision, old_binding.credential_revision);
    assert_eq!(new_binding.placement, old_binding.placement);
    let new_reference = new_binding.credential_id;
    persistence
        .with_reader(move |db| {
            let row = crate::tool_entities::load_optional_record::<
                aura_tool_domain::ToolCredentialAuthorityRecordV1,
                _,
            >(
                db,
                "SELECT record_json,record_hash_json FROM tool_credentials WHERE credential_id=?1",
                [new_reference.to_string()],
            )?
            .expect("original credential metadata");
            assert_eq!(row.allowed_origins, vec!["https://example.test"]);
            assert_eq!(row.scope, aura_tool_domain::ToolCredentialScopeV1::User);
            assert_eq!(row.state, expected_state, "an upgrade cannot revive revoked authority");
            Ok(())
        })
        .expect("upgrade leaves scope and origins unchanged");
    assert!(authority.instance_configurations.iter().any(|c| c.tool_instance_id == current
        && c.configuration == json!({"non_secret_option":"user choice"})));
    assert!(
        persistence
            .materialize_http_credential_target(UtcTimestamp::now())
            .expect("idempotent restart")
            .is_none()
    );
}

#[test]
fn s09_changed_credential_requirements_do_not_auto_bind_and_explain_recovery() {
    let root = tempfile::tempdir().expect("isolated store");
    let persistence = start(&root);
    let old = legacy_with_runtime(Some(ToolRuntimeDescriptorV1::ExternalSource {
        source_contract_key: "synthetic.old.http.without.credentials".into(),
    }));
    persistence
        .dispatch(move |db| {
            let tx = db.transaction()?;
            insert_definition(&tx, &old.definition)?;
            insert_instance(&tx, &old.instance)?;
            tx.commit()?;
            Ok(())
        })
        .expect("old version with different requirements");
    persistence.materialize_http_credential_target(UtcTimestamp::now()).expect("new requirements");
    let id = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .instance
        .tool_instance_id;
    assert_eq!(
        persistence
            .builtin_credential_rebind_reason(id)
            .expect("reason")
            .expect("requires user action"),
        "credential_slot_changed"
    );
    let authority = persistence.tool_authority_snapshot(None, None, None).expect("authority");
    assert!(!authority.instance_credential_bindings.iter().any(|b| b.tool_instance_id == id));
    let projections = persistence
        .with_reader(|db| tool_definition_projection_drafts(db, None))
        .expect("management projection");
    let json = serde_json::to_string(&format!("{projections:?}")).expect("projection");
    assert!(json.contains("rebind_required_reason") && json.contains("credential_slot_changed"));
}

#[test]
fn s09_an_exact_restart_keeps_mutable_builtin_configuration() {
    let root = tempfile::tempdir().expect("isolated store");
    let persistence = start(&root);
    persistence.materialize_http_credential_target(UtcTimestamp::now()).expect("initial release");
    let id = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP")
        .instance
        .tool_instance_id;
    persistence.dispatch(move |db| {
        let tx = db.transaction()?;
        let mut i = crate::tool_entities::load_optional_record::<ToolInstanceRecordV1,_>(&tx,"SELECT record_json,record_hash_json FROM tool_instances WHERE tool_instance_id=?1",[id.to_string()])?.expect("current instance");
        i.enabled = false;
        i.overrides.timeout_ms = Some(60_000);
        i.entity_revision = Revision::new(2);
        let (value,hash) = encode_record(&i)?;
        tx.execute("UPDATE tool_instances SET enabled=0,entity_revision=2,record_json=?2,record_hash_json=?3 WHERE tool_instance_id=?1",params![id.to_string(),value,hash])?;
        tx.commit()?;
        Ok(())
    }).expect("committed user settings");
    assert!(
        persistence
            .materialize_http_credential_target(UtcTimestamp::now())
            .expect("restart accepts user configuration")
            .is_none()
    );
}
