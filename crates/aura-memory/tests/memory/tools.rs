use aura_domain_common::{SessionId, WorkspaceId};
use aura_memory::*;
use serde_json::json;

#[test]
fn repair_first_write_initializes_the_complete_vault_layout() {
    let temp = tempfile::tempdir().expect("TEMP");
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root).expect("workspace");
    let service =
        MemoryService::new(VaultRoots::new(temp.path().join("global"), temp.path().join("cache")));
    let workspace = MemoryWorkspace { workspace_id: WorkspaceId::new(), root: root.clone() };
    let actor = WriteActor { kind: "settings_repair".into(), ..Default::default() };
    let preview: MemoryRepairResult = serde_json::from_value(
        service
            .invoke(
                "memory.repair",
                &json!({"scope":"project","phase":"preview"}),
                &workspace,
                &actor,
            )
            .expect("preview"),
    )
    .expect("result");
    assert!(!root.join(".memories").exists(), "preview must not create the Vault");
    service.invoke("memory.repair", &json!({"scope":"project","phase":"apply","preview_token":preview.preview.preview_token}), &workspace, &actor).expect("apply");
    for directory in [
        "experience",
        "rules",
        "events/archive",
        "archive",
        "views",
        "architecture",
        "commands",
        "known-issues",
    ] {
        assert!(root.join(".memories").join(directory).is_dir(), "missing {directory}");
    }
    assert_eq!(std::fs::read_to_string(root.join(".memories/.gitignore")).expect("ignore"), "*\n");
}

#[test]
fn model_tools_write_then_another_session_reads_and_updates_with_hash_conflict_protection() {
    let temp = tempfile::tempdir().expect("TEMP");
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root).expect("workspace");
    let service =
        MemoryService::new(VaultRoots::new(temp.path().join("global"), temp.path().join("data")));
    let workspace = MemoryWorkspace { workspace_id: WorkspaceId::new(), root };
    let actor = WriteActor {
        kind: "model_tool".into(),
        session_id: Some(SessionId::new()),
        turn_id: Some("turn-fixture-1".into()),
    };
    let result=service.invoke("memory.create",&json!({"scope":"project","items":[{"path":"commands/build.md","category":"commands","body":"# 构建命令\ncargo test --offline\n"}]}),&workspace,&actor).expect("create");
    let entries: MemoryMutationResult = serde_json::from_value(result).expect("receipt");
    let first = &entries.entries[0];
    let second = WriteActor { session_id: Some(SessionId::new()), ..actor.clone() };
    let read: MemoryRead = serde_json::from_value(
        service
            .invoke(
                "memory.read",
                &json!({"target":{"kind":"id","id":first.id}}),
                &workspace,
                &second,
            )
            .expect("another session reads"),
    )
    .expect("read");
    assert_eq!(read.note.metadata.source_sessions, vec![actor.session_id.expect("session")]);
    assert_eq!(read.history[0].actor, "model_tool");
    let update = json!({"scope":"project","items":[{"target":{"kind":"id","id":first.id},"expected_hash":read.hash,"body":"# 构建命令\n新的已验证命令\n"}]});
    service.invoke("memory.update", &update, &workspace, &second).expect("update");
    assert!(matches!(
        service.invoke("memory.update", &update, &workspace, &actor),
        Err(MemoryError::Conflict { .. })
    ));
    let search: MemorySearchResult = serde_json::from_value(
        service
            .invoke(
                "memory.search",
                &json!({"scope":"project","query":"命令"}),
                &workspace,
                &second,
            )
            .expect("Chinese search"),
    )
    .expect("result");
    assert_eq!(search.hits[0].metadata.id, first.id);
    assert!(!temp.path().join("global").exists());
    let other =
        MemoryWorkspace { workspace_id: WorkspaceId::new(), root: temp.path().join("other") };
    std::fs::create_dir(&other.root).expect("other workspace");
    assert!(
        service
            .invoke("memory.read", &json!({"target":{"kind":"id","id":first.id}}), &other, &second)
            .is_err()
    );
}

#[test]
fn repair_is_readonly_until_apply_preserves_body_and_rejects_stale_or_stolen_previews() {
    let temp = tempfile::tempdir().expect("TEMP");
    let root = temp.path().join("workspace");
    let vaultroot = root.join(".memories");
    std::fs::create_dir_all(vaultroot.join("rules")).expect("existing user Vault");
    let original = "# 用户正文\n[[rules/other]]\n不会被自动改写。\n";
    std::fs::write(vaultroot.join("rules/external.md"), original).expect("external note");
    let service =
        MemoryService::new(VaultRoots::new(temp.path().join("global"), temp.path().join("data")));
    let workspace = MemoryWorkspace { workspace_id: WorkspaceId::new(), root };
    let actor = WriteActor { kind: "settings_repair".into(), ..Default::default() };
    let preview: MemoryRepairResult = serde_json::from_value(
        service
            .invoke(
                "memory.repair",
                &json!({"scope":"project","phase":"preview"}),
                &workspace,
                &actor,
            )
            .expect("preview"),
    )
    .expect("result");
    assert!(!preview.applied);
    assert_eq!(
        std::fs::read_to_string(vaultroot.join("rules/external.md")).expect("unchanged"),
        original
    );
    assert!(!vaultroot.join("README.md").exists());
    let thief =
        WriteActor { kind: "model_tool".into(), session_id: Some(SessionId::new()), turn_id: None };
    assert!(service.invoke("memory.repair",&json!({"scope":"project","phase":"apply","preview_token":preview.preview.preview_token}),&workspace,&thief).is_err());
    let preview: MemoryRepairResult = serde_json::from_value(
        service
            .invoke(
                "memory.repair",
                &json!({"scope":"project","phase":"preview"}),
                &workspace,
                &actor,
            )
            .expect("preview again"),
    )
    .expect("result");
    std::fs::write(vaultroot.join("rules/external.md"), format!("{original}new edit\n"))
        .expect("concurrent user edit");
    assert!(service.invoke("memory.repair",&json!({"scope":"project","phase":"apply","preview_token":preview.preview.preview_token}),&workspace,&actor).is_err());
    assert!(!vaultroot.join("README.md").exists());
    let preview: MemoryRepairResult = serde_json::from_value(
        service
            .invoke(
                "memory.repair",
                &json!({"scope":"project","phase":"preview"}),
                &workspace,
                &actor,
            )
            .expect("preview latest"),
    )
    .expect("result");
    service.invoke("memory.repair",&json!({"scope":"project","phase":"apply","preview_token":preview.preview.preview_token}),&workspace,&actor).expect("apply");
    let note =
        parse(&std::fs::read_to_string(vaultroot.join("rules/external.md")).expect("repaired"))
            .expect("valid");
    assert_eq!(note.body, format!("{original}new edit\n"));
    assert!(vaultroot.join("README.md").exists());
    assert!(!vaultroot.join(".gitignore").exists());
    let vault = service.vault(MemoryScope::Project, &workspace).expect("same Vault");
    assert!(
        vault
            .history(Some(note.metadata.id), 10)
            .expect("history")
            .iter()
            .any(|h| h.actor == "settings_repair")
    );
}
