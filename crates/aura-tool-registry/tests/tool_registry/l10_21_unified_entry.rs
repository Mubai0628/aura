//! The unified entry: the request's tool list
//! stays the same for a whole turn, and any other tool of the step's registry is called through
//! `tools.call`. The registry says which tools the entry may reach; the call is rewritten into that
//! tool's own call before anything checks it.
use aura_domain_common::jcs::{CanonicalHashV1, canonical_sha256};
use aura_domain_common::{
    AgentRunId, Revision, SessionId, ToolCallId, ToolId, TurnTaskId, WorkspaceId,
};
use aura_tool_contract::{
    NormalizedToolCallInputStateV1, NormalizedToolCallV1, ToolDefinitionSnapshotRef,
    ToolExposureV1, ToolLoadStateV1, ToolPurposeTextV1, ToolRegistryContractError,
    ToolRegistryScopeV1, ToolRegistrySnapshotV1, ToolSettingsSnapshotV1, entry_callable,
    expand_entry_calls,
};
use aura_tool_registry::agent_release::release;
use aura_tool_registry::{ParentRegistryBuildInputV1, build_parent_registry_snapshot};
use serde_json::{Value, json};

fn id(name: &str) -> ToolId {
    release()
        .expect("verified current release")
        .entry(name)
        .unwrap_or_else(|| panic!("{name} is a built-in tool"))
        .definition
        .tool_id
}

/// A step's registry from the real release, sending only `sent`.
fn snapshot(sent: &[&str]) -> ToolRegistrySnapshotV1 {
    build_parent_registry_snapshot(ParentRegistryBuildInputV1 {
        registry_revision: Revision::INITIAL,
        scope: ToolRegistryScopeV1::Session {
            workspace_id: WorkspaceId::new(),
            session_id: SessionId::new(),
            turn_task_id: TurnTaskId::new(),
            agent_run_id: AgentRunId::new(),
        },
        settings: ToolSettingsSnapshotV1::default(),
        mode_permission_context_hash: CanonicalHashV1::sha256(b"mode"),
        core_exposed_tool_ids: sent.iter().map(|name| id(name)).collect(),
        candidates: release().expect("release").candidates(),
        workspace_pins: vec![],
        session_pins: vec![],
        ephemeral_loads: vec![],
        created_at: "2026-10-22T00:00:00Z".parse().expect("timestamp"),
    })
    .expect("the parent registry builds")
    .snapshot
}

const SENT: &[&str] = &["tools.call", "tools.search", "tools.describe", "fs.read"];

fn entry_call(registry: &ToolRegistrySnapshotV1, arguments: Value) -> NormalizedToolCallV1 {
    let entry = registry
        .catalog_entries
        .iter()
        .find(|entry| entry.canonical_name.as_str() == "tools.call")
        .expect("tools.call is in the catalog");
    NormalizedToolCallV1 {
        tool_call_id: ToolCallId::new(),
        ordinal: 0,
        definition_ref: Some(ToolDefinitionSnapshotRef {
            tool_id: entry.tool_id,
            tool_version: entry.tool_version.clone(),
            registry_snapshot_ref: registry.snapshot_ref.clone(),
        }),
        input: NormalizedToolCallInputStateV1::Ready {
            purpose: ToolPurposeTextV1::new("Read the history").expect("purpose"),
            arguments_hash: canonical_sha256(&arguments).expect("hash"),
            arguments,
        },
        provider_arguments_envelope_hash: CanonicalHashV1::sha256(b"provider bytes"),
    }
}

#[test]
fn every_discoverable_ordinary_tool_is_offered_through_the_entry_and_nothing_else_is() {
    let registry = snapshot(SENT);
    // The tools sent are exactly the schemas; the entry adds none.
    assert_eq!(registry.effective_bindings.len(), registry.schema_set.tools.len());
    assert!(!registry.entry_bindings.is_empty());
    for entry in &registry.catalog_entries {
        let offered = registry.entry_bindings.iter().find(|b| b.tool_id == entry.tool_id);
        let expected = entry.load_state == ToolLoadStateV1::Discoverable
            && entry_callable(entry.canonical_name.as_str());
        assert_eq!(offered.is_some(), expected, "{}", entry.canonical_name);
        if let Some(binding) = offered {
            assert_eq!(binding.exposure, ToolExposureV1::UnifiedEntry);
            assert_eq!(binding.definition_fingerprint, entry.definition_fingerprint);
            assert_eq!(binding.tool_instance_id, entry.tool_instance_id);
        }
    }
    // An ordinary tool is offered; a sent one, the registry tools and the turn-protocol tools
    // are not.
    let offered = |name: &str| registry.entry_bindings.iter().any(|b| b.tool_id == id(name));
    for name in ["git.commit", "http.request", "memory.list_recent", "agent.activity.query"] {
        assert!(offered(name), "{name}");
    }
    for name in [
        "fs.read",
        "tools.load",
        "tools.unload",
        "goal.complete",
        "plan.complete",
        "mode.disposition",
        "normal.complete",
        "query.user_question",
        "task_lineage.reconcile",
        "workspace_init.complete",
        "skills.load",
    ] {
        assert!(!offered(name), "{name}");
    }
    // It survives storage unchanged, and the whole snapshot still validates.
    let stored = serde_json::to_value(&registry).expect("encode");
    assert!(stored["entry_bindings"].is_array());
    let read: ToolRegistrySnapshotV1 = serde_json::from_value(stored).expect("decode");
    assert_eq!(read, registry);
}

#[test]
fn a_snapshot_without_the_entry_keeps_the_shape_and_hash_it_had() {
    let registry = snapshot(SENT);
    let old = ToolRegistrySnapshotV1::new(
        registry.snapshot_ref.tool_registry_snapshot_id,
        registry.snapshot_ref.registry_revision,
        registry.scope.clone(),
        None,
        None,
        registry.eligibility_context_hash.clone(),
        registry.schema_set.tools.clone(),
        registry.catalog_entries.clone(),
        registry.effective_bindings.clone(),
        registry.created_at,
    )
    .expect("a snapshot without entry bindings");
    let stored = serde_json::to_value(&old).expect("encode");
    assert!(stored.get("entry_bindings").is_none(), "the old shape is stored");
    assert_ne!(old.snapshot_ref.content_hash, registry.snapshot_ref.content_hash);
    // Read back as an older version stored it: it validates and offers nothing through the entry.
    let read: ToolRegistrySnapshotV1 = serde_json::from_value(stored).expect("decode");
    assert!(read.entry_bindings.is_empty());
    let mut calls = vec![entry_call(&read, json!({"tool": "git.log", "arguments": {}}))];
    let before = calls.clone();
    expand_entry_calls(&read, &mut calls);
    assert_eq!(calls, before);
}

#[test]
fn the_registry_refuses_an_entry_binding_it_did_not_freeze() {
    let registry = snapshot(SENT);
    let rebuild = |effective: Vec<_>, entry: Vec<_>| {
        ToolRegistrySnapshotV1::with_entry_bindings(
            registry.snapshot_ref.tool_registry_snapshot_id,
            registry.snapshot_ref.registry_revision,
            registry.scope.clone(),
            None,
            None,
            registry.eligibility_context_hash.clone(),
            registry.schema_set.tools.clone(),
            registry.catalog_entries.clone(),
            effective,
            entry,
            registry.created_at,
        )
    };
    rebuild(registry.effective_bindings.clone(), registry.entry_bindings.clone())
        .expect("the frozen bindings rebuild");
    let mismatch = Some(ToolRegistryContractError::ExposureMismatch);
    // A tool that is sent cannot also be offered through the entry.
    let sent = registry.effective_bindings[0].clone();
    let mut entry = registry.entry_bindings.clone();
    entry.push(aura_tool_contract::EffectiveToolBindingSnapshot {
        exposure: ToolExposureV1::UnifiedEntry,
        ..sent
    });
    entry.sort_by_key(|b| b.tool_id);
    assert_eq!(rebuild(registry.effective_bindings.clone(), entry).err(), mismatch.clone());
    // An entry binding is the entry's exposure, and only for a tool the entry may reach.
    let mut entry = registry.entry_bindings.clone();
    entry[0].exposure = ToolExposureV1::EphemeralLoad;
    assert_eq!(rebuild(registry.effective_bindings.clone(), entry).err(), mismatch.clone());
    let protocol = snapshot(&["tools.call"]);
    let goal = protocol
        .catalog_entries
        .iter()
        .find(|e| e.canonical_name.as_str() == "goal.complete")
        .expect("goal.complete is discoverable outside Goal");
    assert_eq!(goal.load_state, ToolLoadStateV1::Discoverable);
    let mut entry = registry.entry_bindings.clone();
    entry.push(aura_tool_contract::EffectiveToolBindingSnapshot {
        tool_id: goal.tool_id,
        tool_version: goal.tool_version.clone(),
        tool_instance_id: goal.tool_instance_id,
        source_kind: goal.source,
        exposure: ToolExposureV1::UnifiedEntry,
        definition_fingerprint: goal.definition_fingerprint.clone(),
        source_contract_fingerprint: goal.source_contract_fingerprint.clone(),
    });
    entry.sort_by_key(|b| b.tool_id);
    assert_eq!(rebuild(registry.effective_bindings.clone(), entry).err(), mismatch.clone());
    // A sent tool never carries the entry's exposure.
    let mut effective = registry.effective_bindings.clone();
    effective[0].exposure = ToolExposureV1::UnifiedEntry;
    assert_eq!(rebuild(effective, registry.entry_bindings.clone()).err(), mismatch);
}

#[test]
fn a_call_through_the_entry_becomes_the_named_tools_own_call() {
    let registry = snapshot(SENT);
    let log = id("git.log");
    let target_arguments = json!({"limit": 3});
    let mut calls = vec![
        entry_call(&registry, json!({"tool": "git.log", "arguments": target_arguments})),
        // Its tool_id and the name a Provider saw name it too.
        entry_call(&registry, json!({"tool": log.to_string(), "arguments": {}})),
        entry_call(
            &registry,
            json!({"tool": format!("git_log__{}", log.ulid().to_string().to_ascii_lowercase()), "arguments": {}}),
        ),
    ];
    let original = calls.clone();
    expand_entry_calls(&registry, &mut calls);
    for (call, made) in calls.iter().zip(&original) {
        let reference = call.definition_ref.as_ref().expect("bound");
        assert_eq!(reference.tool_id, log);
        assert_eq!(reference.registry_snapshot_ref, registry.snapshot_ref);
        assert_eq!(call.tool_call_id, made.tool_call_id);
        assert_eq!(call.ordinal, made.ordinal);
        assert_eq!(call.provider_arguments_envelope_hash, made.provider_arguments_envelope_hash);
    }
    let NormalizedToolCallInputStateV1::Ready { purpose, arguments, arguments_hash } =
        &calls[0].input
    else {
        panic!("ready input");
    };
    assert_eq!(purpose.as_str(), "Read the history");
    assert_eq!(arguments, &target_arguments);
    assert_eq!(arguments_hash, &canonical_sha256(&target_arguments).expect("hash"));
    // A tool that is sent can be named too: it is the same tool with the same checks.
    let mut sent =
        vec![entry_call(&registry, json!({"tool": "fs.read", "arguments": {"path": "a"}}))];
    expand_entry_calls(&registry, &mut sent);
    assert_eq!(sent[0].definition_ref.as_ref().map(|r| r.tool_id), Some(id("fs.read")));
}

#[test]
fn a_call_naming_nothing_the_entry_may_reach_stays_the_entrys_and_runs_nothing() {
    let registry = snapshot(SENT);
    let unchanged = |arguments: Value| {
        let mut calls = vec![entry_call(&registry, arguments.clone())];
        let before = calls.clone();
        expand_entry_calls(&registry, &mut calls);
        assert_eq!(calls, before, "{arguments}");
        assert!(aura_tool_contract::is_entry_call(&registry, &calls[0]));
    };
    // Unknown names, the protocol and registry tools, and the entry itself.
    for tool in [
        "git.nothing",
        "goal.complete",
        "plan.complete",
        "mode.disposition",
        "query.user_question",
        "tools.load",
        "tools.call",
        "workspace_init.complete",
        "",
    ] {
        unchanged(json!({"tool": tool, "arguments": {}}));
    }
    // Input that is not exactly {tool, arguments}.
    unchanged(json!({"tool": "git.log"}));
    unchanged(json!({"tool": "git.log", "arguments": [1]}));
    unchanged(json!({"tool": "git.log", "arguments": {}, "also": 1}));
    unchanged(json!({"name": "git.log", "arguments": {}}));
    // A call bound to another snapshot is left for admission to refuse.
    let other = snapshot(SENT);
    let mut calls = vec![entry_call(&other, json!({"tool": "git.log", "arguments": {}}))];
    let before = calls.clone();
    expand_entry_calls(&registry, &mut calls);
    assert_eq!(calls, before);
}

#[test]
fn the_entry_is_published_with_its_own_identity_and_says_how_it_is_used() {
    let release = release().expect("release");
    let entry = release.entry("tools.call").expect("tools.call is released");
    assert_eq!(entry.definition.tool_id.to_string(), "tol_01M2DG020980080021AN942023");
    let input = entry.definition.input_schema.document();
    let mut required: Vec<String> =
        serde_json::from_value(input["required"].clone()).expect("required");
    required.sort();
    assert_eq!(required, ["arguments", "tool"]);
    assert_eq!(input["additionalProperties"], json!(false));
    assert!(entry.definition.model_description.contains("canonical_name from tools.search"));
    // tools.load and tools.unload say their change applies from the next turn; tools.search
    // names the entry.
    for name in ["tools.load", "tools.unload"] {
        let description = &release.entry(name).expect(name).definition.model_description;
        assert!(description.contains("from the next turn"), "{name}: {description}");
    }
    let search = &release.entry("tools.search").expect("search").definition;
    assert!(search.model_description.contains("tools.call"));
    // The search input is the signed one.
    let signed = aura_tool_registry::embedded_builtin_tool_definition_set_v1()
        .expect("signed set")
        .definition("tools.search")
        .expect("signed search")
        .input_schema
        .clone();
    assert_eq!(search.input_schema, signed);
}
