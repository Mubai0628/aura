use aura_tool_contract::ToolSourceKindV1;
use aura_tool_registry::agent_release::*;

#[test]
fn closed_release_has_complete_unique_stable_identities() {
    let generated = generate_manifest().expect("all domain schemas form valid Tool definitions");
    let embedded = release().expect("embedded release matches generated schemas");
    assert_eq!(&generated, embedded.manifest());
    // The count is a completeness check on the closed release, so it moves when
    // the release legitimately gains a tool and the new entry is named below.
    assert_eq!(generated.entries.len(), 109);
    // The built-in browser: sixteen tools, all the parent's (a child agent has no browser).
    // Acting on a page may change something there; reading it, scrolling and hovering do not,
    // and asking the user for help leaves what they do to them.
    for (name, acts) in [
        ("browser.open", false),
        ("browser.navigate", false),
        ("browser.snapshot", false),
        ("browser.screenshot", false),
        ("browser.click", true),
        ("browser.type", true),
        ("browser.press", true),
        ("browser.scroll", false),
        ("browser.hover", false),
        ("browser.select", true),
        ("browser.wait", false),
        ("browser.console", false),
        ("browser.close", false),
        ("browser.request_user", false),
        ("browser.evaluate", true),
        ("browser.network", false),
        // Read through DevTools; nothing on the page changes.
        ("browser.inspect", false),
        ("browser.performance", false),
    ] {
        let entry = embedded.entry(name).expect("L13 browser tool is published");
        assert_eq!(entry.definition.audience, aura_tool_contract::ToolAudienceV1::ParentAgentOnly);
        assert_eq!(
            entry.definition.execution_semantics.may_create_persistent_effects, acts,
            "{name}"
        );
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed, "{name}");
    }
    // The user's own Chrome: fifteen tools, all the parent's. Opening, navigating, clicking, typing,
    // pressing, choosing and closing may change something in the user's signed-in browser.
    for (name, acts) in [
        ("chrome.tabs", false),
        ("chrome.open", true),
        ("chrome.navigate", true),
        ("chrome.snapshot", false),
        ("chrome.screenshot", false),
        ("chrome.click", true),
        ("chrome.type", true),
        ("chrome.press", true),
        ("chrome.scroll", false),
        ("chrome.hover", false),
        ("chrome.select", true),
        ("chrome.wait", false),
        ("chrome.console", false),
        ("chrome.close", true),
        ("chrome.request_user", false),
    ] {
        let entry = embedded.entry(name).expect("L14 Chrome tool is published");
        assert_eq!(entry.definition.audience, aura_tool_contract::ToolAudienceV1::ParentAgentOnly);
        // Listing the shares reads Core's own record; every other tool asks Chrome.
        use aura_security_contract::ResolvedCapability as C;
        let capabilities = &entry.definition.capabilities;
        if name == "chrome.tabs" {
            assert_eq!(capabilities, &[C::AgentTaskRead]);
        } else {
            assert!(
                !capabilities.is_empty()
                    && capabilities
                        .iter()
                        .all(|c| matches!(c, C::ChromeRead | C::ChromeInteraction)),
                "{name}: {capabilities:?}"
            );
        }
        assert_eq!(
            entry.definition.execution_semantics.may_create_persistent_effects, acts,
            "{name}"
        );
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed, "{name}");
    }
    // The unified entry: parent only, and it runs nothing itself - admission rewrites its
    // calls into the tool they name.
    let call = embedded.entry("tools.call").expect("L10.21 entry is published");
    assert!(!call.definition.execution_semantics.may_create_persistent_effects);
    assert_eq!(call.definition.audience, aura_tool_contract::ToolAudienceV1::ParentAgentOnly);
    // The user's Skills: four reads, and a load and an unload that change only this turn.
    for (name, writes) in [
        ("skills.search", false),
        ("skills.describe", false),
        ("skills.load", true),
        ("skills.unload", true),
        ("skills.list_loaded", false),
        ("skills.read_resource", false),
    ] {
        let entry = embedded.entry(name).expect("L09 Skill tool is published");
        assert_eq!(entry.definition.execution_semantics.may_create_persistent_effects, writes);
    }
    // Workspace prompt commands: two reads, and a save and a delete that write files.
    for (name, writes) in [
        ("commands.list", false),
        ("commands.describe", false),
        ("commands.save", true),
        ("commands.delete", true),
    ] {
        let entry = embedded.entry(name).expect("L06 command tool is published");
        assert_eq!(entry.definition.execution_semantics.may_create_persistent_effects, writes);
    }
    // Adds `workspace_init.complete`: parent only, a closed workspace write.
    let init = embedded.entry("workspace_init.complete").expect("L06 init tool is published");
    assert_eq!(
        init.definition.capabilities,
        vec![
            aura_security_contract::ResolvedCapability::WorkspaceFileWrite,
            aura_security_contract::ResolvedCapability::WorkspaceFileCreate
        ]
    );
    assert!(init.definition.execution_semantics.may_create_persistent_effects);
    assert!(!init.definition.execution_semantics.automatic_retry_allowed);
    for name in ["git.status", "git.diff", "git.log"] {
        let entry = embedded.entry(name).expect("B1 read tool is published");
        assert_eq!(
            entry.definition.capabilities,
            vec![aura_security_contract::ResolvedCapability::GitRead]
        );
        assert!(!entry.definition.execution_semantics.may_create_persistent_effects);
        assert!(entry.definition.execution_semantics.repeat_is_idempotent);
    }
    for name in ["git.commit", "git.branch"] {
        let entry = embedded.entry(name).expect("B2 local write tool is published");
        assert_eq!(
            entry.definition.capabilities,
            vec![aura_security_contract::ResolvedCapability::GitWrite]
        );
        assert!(entry.definition.execution_semantics.may_create_persistent_effects);
        assert!(!entry.definition.execution_semantics.repeat_is_idempotent);
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed);
    }
    for (name, capability) in [
        ("git.sync", aura_security_contract::ResolvedCapability::GitRemoteRead),
        ("git.push", aura_security_contract::ResolvedCapability::GitRemoteWrite),
    ] {
        let entry = embedded.entry(name).expect("B3 remote tool is published");
        assert_eq!(entry.definition.capabilities, vec![capability]);
        assert!(entry.definition.execution_semantics.may_create_persistent_effects);
        assert!(!entry.definition.execution_semantics.repeat_is_idempotent);
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed);
    }
    for name in [
        "terminal.open",
        "terminal.write",
        "terminal.read",
        "terminal.request_sensitive_input",
        "terminal.close",
    ] {
        assert!(embedded.entry(name).is_some(), "missing L02 tool {name}");
    }
    assert!(embedded.entry("fs.list").is_some());
    assert!(embedded.entry("fs.find").is_some());
    assert!(embedded.entry("http.request").is_some());
    assert!(embedded.entry("http.download").is_some());
    for entry in &generated.entries {
        assert_eq!(entry.definition.tool_id, entry.instance.tool_id);
        assert_eq!(entry.definition.tool_version, entry.instance.tool_version);
        assert_eq!(entry.definition.source_kind, ToolSourceKindV1::BuiltIn);
        assert!(!entry.definition.execution_semantics.automatic_retry_allowed);
    }
    assert!(embedded.entry("shell.powershell").is_some());
    assert!(embedded.entry("shell.cmd").is_some());
    assert!(embedded.entry("web.search").is_some());
    assert!(embedded.entry("task_lineage.review_completion").is_some());
    let activity = embedded.entry("agent.activity.query").expect("the read-only activity query");
    assert!(!activity.definition.execution_semantics.may_create_persistent_effects);
    assert!(activity.definition.execution_semantics.repeat_is_idempotent);
    assert!(embedded.entry("package.install").is_none());
}

/// A schema with its descriptions removed: what the input accepts.
fn without_descriptions(schema: &serde_json::Value) -> serde_json::Value {
    match schema {
        serde_json::Value::Object(map) => map
            .iter()
            .filter(|(key, _)| *key != "description")
            .map(|(key, value)| (key.clone(), without_descriptions(value)))
            .collect(),
        serde_json::Value::Array(items) => items.iter().map(without_descriptions).collect(),
        other => other.clone(),
    }
}

#[test]
fn management_schema_versions_preserve_old_identity_without_reinterpreting_old_definitions() {
    let release = release().expect("current embedded release");
    let at = "2026-09-09T00:00:00Z".parse().expect("fixed timestamp");
    for legacy in aura_tool_registry::embedded_builtin_tool_definition_set_v1()
        .expect("original built-in release")
        .entries()
    {
        let entry = release
            .entry(legacy.execution_binding.builtin_handler().builtin_key())
            .expect("legacy tool remains in the current release");
        let original = legacy.definition_record(at).expect("legacy definition");
        if matches!(
            entry.handler.name(),
            "tools.describe"
                | "tools.list_loaded"
                | "tools.load"
                | "tools.unload"
                | "tools.search"
                | "agent.activity.query"
        ) {
            assert_eq!(entry.definition.tool_id, original.tool_id);
            assert_eq!(original.tool_version.definition_revision.value(), 1);
            // Tools.load and tools.unload apply from the next turn (revision 3) and say a load does
            // not change this turn's list (revision 4), tools.search names tools.call (revision 2),
            // and the activity query lists the calling session's own activities (revision 4).
            let revision = match entry.handler.name() {
                "tools.load" | "tools.unload" => 4,
                "agent.activity.query" => 4,
                _ => 2,
            };
            assert_eq!(entry.definition.tool_version.definition_revision.value(), revision);
            assert_ne!(
                entry.instance.tool_instance_id,
                legacy.instance_record(at).tool_instance_id
            );
            let properties = &entry.definition.input_schema.document()["properties"];
            assert!(properties.get("registry_snapshot_ref").is_none());
            assert!(properties.get("definition_ref").is_none());
            if matches!(entry.handler.name(), "agent.activity.query" | "tools.search") {
                // Both model definitions bind the caller from its run (the search names no
                // caller at all). The Agent release changes descriptions and the version, not
                // what the input accepts.
                assert_eq!(
                    without_descriptions(entry.definition.input_schema.document()),
                    without_descriptions(original.input_schema.document())
                );
                assert_eq!(entry.definition.result_contract, original.result_contract);
                assert_ne!(entry.definition.model_description, original.model_description);
                assert_ne!(
                    entry.definition.definition_fingerprint,
                    original.definition_fingerprint
                );
                assert!(properties.get("session_id").is_none());
                assert!(properties.get("workspace_id").is_none());
            } else {
                assert_ne!(entry.definition.input_schema, original.input_schema);
            }
        } else {
            assert_eq!(entry.definition, original);
            assert_eq!(entry.instance, legacy.instance_record(at));
        }
    }
}
