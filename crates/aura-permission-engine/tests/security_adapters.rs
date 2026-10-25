use aura_domain_common::jcs::{CanonicalSha256HashV1, canonical_sha256};
use aura_domain_common::{ContentHash, PermissionGrantId, UtcTimestamp};
use aura_permission_domain::{
    OperationSecurityPlan, PermissionGrant, ResolvedScope, SecuritySource,
};
use aura_permission_engine::{PermissionEvaluation, matching_grant};
use aura_security_contract::{
    PermissionGrantLifetime, PermissionMode, PermissionSnapshot, ResourceDescriptor,
    SecuritySourceRef, TrustState,
};
use serde_json::{Value, json};

mod support;

fn grant_for(plan: &OperationSecurityPlan, snapshot: &mut PermissionSnapshot) -> PermissionGrant {
    let grant = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Session,
        workspace_id: snapshot.workspace_id,
        session_id: plan.identity.session_id,
        turn_task_id: plan.identity.turn_task_id,
        source: plan.source.clone(),
        source_fingerprint: plan.source_fingerprint.clone(),
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: None,
        created_at: snapshot.created_at,
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    snapshot.active_grant_ids.push(grant.id);
    grant
}

fn matches(
    plan: &OperationSecurityPlan,
    snapshot: &PermissionSnapshot,
    grant: &PermissionGrant,
) -> bool {
    matching_grant(&PermissionEvaluation {
        plan,
        snapshot: snapshot.clone(),
        source_enabled: true,
        trust_state: &TrustState::SystemTrusted,
        grants: std::slice::from_ref(grant),
        turn_denies: &[],
        available_sandbox_modes: &[],
        network_control_available: false,
        allow_unrestricted_inline_scripts_in_full_access: false,
        now: snapshot.created_at,
    })
    .is_some()
}

fn id(prefix: &str) -> String {
    format!("{prefix}_01ARZ3NDEKTSV4RRFFQ69G5FAV")
}

fn hash(label: &str) -> Value {
    serde_json::to_value(CanonicalSha256HashV1::sha256(label.as_bytes())).expect("digest wire")
}

#[test]
fn tool_reference_preserves_key_and_invalidates_grants_for_every_exact_binding_field() {
    let (plan, mut snapshot) = support::fixture(PermissionMode::ReadOnly);
    let SecuritySource::Tool { tool } = &plan.source else { panic!("tool source") };
    assert_eq!(
        plan.source.stable_key(),
        format!("tool:{}", canonical_sha256(tool).expect("canonical Tool source").hex())
    );
    let grant = grant_for(&plan, &mut snapshot);
    assert!(matches(&plan, &snapshot, &grant));
    for (pointer, replacement) in [
        ("/tool/definition_ref/tool_id", json!(id("tol"))),
        ("/tool/tool_instance_id", json!(id("tin"))),
        ("/tool/definition_ref/tool_version/definition_revision", json!("2")),
        ("/tool/definition_ref/tool_version/semantic_version", json!("2.0.0")),
        ("/tool/definition_ref/registry_snapshot_ref/tool_registry_snapshot_id", json!(id("trs"))),
        ("/tool/definition_ref/registry_snapshot_ref/registry_revision", json!("2")),
        ("/tool/definition_ref/registry_snapshot_ref/content_hash", hash("new registry")),
        ("/tool/definition_fingerprint", hash("new definition")),
        (
            "/tool/source_contract_fingerprint/signed_builtin_tool_contract_fingerprint",
            hash("new source"),
        ),
    ] {
        let mut value = serde_json::to_value(&plan.source).expect("source wire");
        *value.pointer_mut(pointer).expect(pointer) = replacement;
        let mut changed = plan.clone();
        changed.source = serde_json::from_value(value).expect(pointer);
        assert_ne!(
            plan.source.security_source_ref(),
            changed.source.security_source_ref(),
            "{pointer}"
        );
        assert!(!matches(&changed, &snapshot, &grant), "old grant accepted {pointer}");
    }
    let mut changed = plan.clone();
    changed.source_fingerprint = Some(ContentHash::blake3(b"new trust revision"));
    assert!(!matches(&changed, &snapshot, &grant));
}

fn conformance() -> SecuritySource {
    serde_json::from_value(json!({
        "source_kind": "tool_conformance",
        "conformance": {
            "tool_conformance_run_id": id("tcr"),
            "tool_draft_id": id("tdr"),
            "draft_revision": "1",
            "draft_snapshot_hash": hash("draft"),
            "tool_key": "read_file",
            "conformance_case_name": "smoke",
            "canonical_manifest_hash": hash("manifest"),
            "runtime_closure_hash": hash("closure"),
            "tool_contract_fingerprint": {
                "package_runtime_closure_hash": hash("closure"),
                "fingerprint": hash("contract")
            },
            "conformance_binding_hash": hash("binding")
        }
    }))
    .expect("synthetic Conformance source")
}

#[test]
fn conformance_identity_is_not_installed_tool_authority_and_binds_the_whole_run() {
    let (mut plan, mut snapshot) = support::fixture(PermissionMode::ReadOnly);
    plan.source = conformance();
    let SecuritySource::ToolConformance { conformance } = &plan.source else {
        panic!("conformance")
    };
    let digest = canonical_sha256(conformance).expect("canonical Conformance source");
    assert_eq!(plan.source.stable_key(), format!("tool_conformance:{}", digest.hex()));
    assert!(!plan.source.security_source_ref().requires_trust());
    assert_ne!(
        plan.source.security_source_ref(),
        SecuritySourceRef::BuiltInTool { identity_hash: digest }
    );
    let grant = grant_for(&plan, &mut snapshot);
    assert!(matches(&plan, &snapshot, &grant));
    for (pointer, replacement) in [
        (
            "/conformance/tool_conformance_run_id",
            json!(aura_domain_common::ToolConformanceRunId::new()),
        ),
        ("/conformance/tool_draft_id", json!(aura_domain_common::ToolDraftId::new())),
        ("/conformance/draft_revision", json!("2")),
        ("/conformance/draft_snapshot_hash", hash("changed draft")),
        ("/conformance/tool_key", json!("write_file")),
        ("/conformance/conformance_case_name", json!("other")),
        ("/conformance/canonical_manifest_hash", hash("changed manifest")),
        ("/conformance/runtime_closure_hash", hash("changed closure")),
        ("/conformance/tool_contract_fingerprint/fingerprint", hash("changed contract")),
        (
            "/conformance/tool_contract_fingerprint/package_runtime_closure_hash",
            hash("changed contract closure"),
        ),
        ("/conformance/conformance_binding_hash", hash("changed binding")),
    ] {
        let mut value = serde_json::to_value(&plan.source).expect("source wire");
        *value.pointer_mut(pointer).expect(pointer) = replacement;
        let mut changed = plan.clone();
        changed.source = serde_json::from_value(value).expect(pointer);
        assert!(!matches(&changed, &snapshot, &grant), "old grant accepted {pointer}");
    }
}

#[test]
fn resource_adapters_preserve_each_scope_and_reject_changed_file_identity_or_authority() {
    let conformance_value =
        serde_json::to_value(conformance()).expect("source wire")["conformance"].clone();
    let path = json!({"display": "C:\\work\\file.txt", "utf16": [67,58,92,119,111,114,107,92,102,105,108,101,46,116,120,116]});
    let cases = vec![
        json!({"scope_kind":"workspace_root","workspace_id":id("wsp")}),
        json!({"scope_kind":"workspace_path","workspace_id":id("wsp"),"canonical_relative_path":"src/main.rs"}),
        json!({"scope_kind":"session_authorized_file","file_reference_id":id("fre")}),
        json!({"scope_kind":"external_path","canonical_path":path,"identity":{"volume_serial_number":"vol1","file_id":"file1"},"recursive":false}),
        json!({"scope_kind":"external_grant","grant_id":id("xgr")}),
        json!({"scope_kind":"network_origin","scheme":"https","host":"example.test","port":443}),
        json!({"scope_kind":"external_service","service_id":"service1","object_ref":"object1"}),
        json!({"scope_kind":"system_path","canonical_path":path}),
        json!({"scope_kind":"sensitive_user_path","canonical_path":path}),
        json!({"scope_kind":"registry_key","hive":"HKCU","path":"Software\\Aura"}),
        json!({"scope_kind":"git_repository","workspace_id":id("wsp")}),
        json!({"scope_kind":"git_remote","remote":"origin"}),
        json!({"scope_kind":"browser_page","page_ref":"page1"}),
        json!({"scope_kind":"package_runtime","package":{"tool_package_id":id("tpk"),"package_version":"1.2.3","runtime_closure_hash":hash("closure")}}),
        json!({"scope_kind":"tool_conformance_runtime","conformance":{"conformance":conformance_value}}),
        json!({"scope_kind":"tool_registry_snapshot","registry_snapshot_ref":{"tool_registry_snapshot_id":id("trs"),"registry_revision":"1","content_hash":hash("registry")}}),
        json!({"scope_kind":"turn_task_tool_exposure","session_id":id("ses"),"turn_task_id":id("trn"),"agent_run_id":id("agt")}),
        json!({"scope_kind":"private_network","class":"loopback"}),
        json!({"scope_kind":"environment_scope","scope":"process"}),
    ];
    let scopes: Vec<ResolvedScope> = cases
        .into_iter()
        .map(|value| {
            serde_json::from_value(value.clone()).unwrap_or_else(|e| panic!("{value}: {e}"))
        })
        .collect();
    for (index, scope) in scopes.iter().enumerate() {
        let descriptor = scope.resource_descriptor();
        let wire = serde_json::to_value(&descriptor).expect("descriptor wire");
        assert_eq!(
            serde_json::from_value::<ResourceDescriptor>(wire.clone())
                .expect("descriptor round trip"),
            descriptor
        );
        let legacy = serde_json::to_value(scope).expect("scope wire");
        if wire.get("resource_fingerprint").is_some() {
            assert_eq!(
                wire["resource_fingerprint"],
                serde_json::to_value(canonical_sha256(scope).expect("canonical scope"))
                    .expect("digest wire")
            );
        } else if matches!(
            wire.get("scope_kind").and_then(serde_json::Value::as_str),
            Some("system_path" | "sensitive_user_path")
        ) {
            // A protected location's descriptor carries the identity execution must re-check,
            // so it is a wider wire form than the scope it came from. The path must still be
            // the scope's own, and the identity is absent here because a resolved scope does
            // not record one — the adapter must not invent it.
            assert_eq!(wire["canonical_path"], legacy["canonical_path"]);
            // Unbound identity is omitted, not stored as null: a null key would change the
            // canonical bytes of operations saved before the field existed.
            assert!(wire.get("identity").is_none());
            assert!(wire.get("parent_identity").is_none());
        } else {
            assert_eq!(wire, legacy);
        }
        for other in &scopes[index + 1..] {
            assert_ne!(descriptor, other.resource_descriptor());
        }
    }
    let (mut plan, mut snapshot) = support::fixture(PermissionMode::ReadOnly);
    plan.scopes = scopes;
    let grant = grant_for(&plan, &mut snapshot);
    assert!(matches(&plan, &snapshot, &grant));
    for (pointer, replacement) in [
        ("/3/identity/file_id", json!("replacement-file")),
        ("/3/identity/volume_serial_number", json!("different-volume")),
        ("/3/recursive", json!(true)),
        ("/3/canonical_path/utf16/3", json!(120)),
        ("/5/host", json!("different.test")),
        ("/15/registry_snapshot_ref/registry_revision", json!("2")),
        ("/14/conformance/conformance/conformance_binding_hash", hash("different binding")),
        ("/16/turn_task_id", json!(aura_domain_common::TurnTaskId::new())),
    ] {
        let mut value = serde_json::to_value(&plan.scopes).expect("scope wire");
        *value.pointer_mut(pointer).expect(pointer) = replacement;
        let mut changed = plan.clone();
        changed.scopes = serde_json::from_value(value).expect(pointer);
        assert!(!matches(&changed, &snapshot, &grant), "old grant accepted {pointer}");
    }
}

#[test]
fn tool_origin_adapter_keeps_all_six_trust_classes_and_rejects_mismatched_fingerprints() {
    let source = support::built_in_tool_source();
    assert!(!source.security_source_ref().requires_trust());
    for (kind, fingerprint) in [
        (
            "user_created",
            json!({"tool_contract_fingerprint":{"package_runtime_closure_hash":hash("closure"),"fingerprint":hash("user")}}),
        ),
        (
            "model_created",
            json!({"tool_contract_fingerprint":{"package_runtime_closure_hash":hash("closure"),"fingerprint":hash("model")}}),
        ),
        (
            "imported",
            json!({"tool_contract_fingerprint":{"package_runtime_closure_hash":hash("closure"),"fingerprint":hash("imported")}}),
        ),
        ("mcp", json!({"normalized_mcp_tool_contract_fingerprint":hash("mcp")})),
        ("plugin", json!({"normalized_plugin_tool_contract_fingerprint":hash("plugin")})),
    ] {
        let mut value = serde_json::to_value(&source).expect("source wire");
        value["tool"]["source_kind"] = json!(kind);
        // Kind alone cannot turn a signed Built-in fingerprint into an extension.
        assert!(serde_json::from_value::<SecuritySource>(value.clone()).is_err());
        value["tool"]["source_contract_fingerprint"] = fingerprint;
        value["tool"]["source_contract_fingerprint"]["source_kind"] = json!(kind);
        let extension: SecuritySource = serde_json::from_value(value).expect(kind);
        let reference = extension.security_source_ref();
        assert!(reference.requires_trust());
        assert!(!reference.trust_is_effective(None, &TrustState::Untrusted));
        assert_ne!(reference, source.security_source_ref());
    }
}

#[test]
fn trust_classes_keep_revocation_and_exact_fingerprint_semantics() {
    let fingerprint = ContentHash::blake3(b"approved source");
    let changed = ContentHash::blake3(b"changed source");
    let trusted =
        TrustState::Trusted { fingerprint: fingerprint.clone(), trusted_at: UtcTimestamp::now() };
    for (source, requires_trust, key) in [
        (SecuritySource::UserInterface, false, "user_interface"),
        (SecuritySource::Hook { hook_id: "id".into() }, true, "hook:id"),
        (SecuritySource::McpServer { server_id: "id".into() }, true, "mcp:id"),
        (SecuritySource::Skill { skill_id: "id".into() }, true, "skill:id"),
        (SecuritySource::Plugin { plugin_id: "id".into() }, true, "plugin:id"),
        (SecuritySource::SystemComponent { component_id: "id".into() }, false, "system:id"),
    ] {
        let reference = source.security_source_ref();
        assert_eq!(reference.stable_key(), key);
        assert_eq!(reference.requires_trust(), requires_trust);
        assert!(!reference.trust_is_effective(Some(&fingerprint), &TrustState::Revoked));
        assert!(reference.trust_is_effective(Some(&fingerprint), &trusted));
        assert_eq!(reference.trust_is_effective(Some(&changed), &trusted), !requires_trust);
        assert_eq!(reference.trust_is_effective(None, &trusted), !requires_trust);
        assert_eq!(reference.trust_is_effective(None, &TrustState::Untrusted), !requires_trust);
    }
}
