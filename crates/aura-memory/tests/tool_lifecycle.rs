use aura_domain_common::{WorkspaceId, WorkspacePath};
use aura_memory::*;
use serde_json::{Value, json};

#[test]
fn lifecycle_links_states_validation_and_hash_checked_batch_delete_use_the_same_vault_writer() {
    let temp = tempfile::tempdir().expect("TEMP");
    let workspace =
        MemoryWorkspace { workspace_id: WorkspaceId::new(), root: temp.path().join("workspace") };
    std::fs::create_dir(&workspace.root).expect("workspace");
    let service =
        MemoryService::new(VaultRoots::new(temp.path().join("global"), temp.path().join("cache")));
    let actor = WriteActor { kind: "model_tool".into(), ..Default::default() };
    let invoke = |name: &str, args: Value| service.invoke(name, &args, &workspace, &actor);
    let created = invoke(
        "memory.create",
        json!({"scope":"project","items":[
            {"path":"rules/a.md","category":"rules","body":"# 约定 A\n第一份临时记录\n"},
            {"path":"rules/b.md","category":"rules","body":"# 约定 B\n第二份临时记录\n"}
        ]}),
    )
    .expect("atomic create");
    let a = created["entries"][0]["id"].clone();
    let b = created["entries"][1]["id"].clone();
    let global=invoke("memory.create",json!({"scope":"global","items":[{"path":"rules/foreign.md","category":"rules","body":"# foreign\n"}]})).expect("separate Vault");
    let before = std::fs::read(workspace.root.join(".memories/rules/a.md")).expect("original");
    assert!(invoke("memory.link",json!({"scope":"project","source":a,"target":global["entries"][0]["id"],"relation":"related"})).is_err());
    assert_eq!(
        before,
        std::fs::read(workspace.root.join(".memories/rules/a.md")).expect("unchanged")
    );
    invoke("memory.link", json!({"scope":"project","source":a,"target":b,"relation":"conflicts"}))
        .expect("symmetric conflict");
    let search = invoke("memory.search", json!({"scope":"project","query":"约定"}))
        .expect("search conflict");
    assert_eq!(search["hits"].as_array().expect("hits").len(), 2);
    assert!(
        search["hits"]
            .as_array()
            .expect("hits")
            .iter()
            .all(|h| h["metadata"]["status"] == "conflicted"
                && h["metadata"]["conflicts_with_ids"].as_array().expect("relation").len() == 1)
    );
    invoke("memory.forget", json!({"scope":"project","targets":[{"kind":"id","id":a}]}))
        .expect("forget");
    invoke("memory.archive", json!({"scope":"project","targets":[{"kind":"id","id":b}]}))
        .expect("archive");
    assert!(workspace.root.join(".memories/rules/a.md").is_file());
    assert!(workspace.root.join(".memories/rules/b.md").is_file());
    assert!(
        invoke("memory.list_recent", json!({"scope":"project"})).expect("recent")["hits"]
            .as_array()
            .expect("hits")
            .is_empty()
    );
    let validation =
        invoke("memory.validate", json!({"scope":"project"})).expect("readonly validation");
    assert_eq!(validation["valid"], 2);
    assert!(validation["invalid"].as_array().expect("invalid").is_empty());
    let read_a = invoke("memory.read", json!({"target":{"kind":"id","id":a}}))
        .expect("inactive still addressable");
    let read_b = invoke(
        "memory.read",
        json!({"target":{"kind":"path","scope":"project","path":"rules/b.md"}}),
    )
    .expect("scoped path");
    let mut deletion = json!({"scope":"project","items":[{"target":{"kind":"id","id":a},"expected_hash":read_a["hash"]},{"target":{"kind":"id","id":b},"expected_hash":"0".repeat(64)}]});
    assert!(invoke("memory.delete", deletion.clone()).is_err());
    assert!(
        workspace.root.join(".memories/rules/a.md").is_file(),
        "a later conflict must not partially delete"
    );
    deletion["items"][1]["expected_hash"] = read_b["hash"].clone();
    invoke("memory.delete", deletion).expect("batch delete");
    let vault = service.vault(MemoryScope::Project, &workspace).expect("same writer");
    assert_eq!(
        vault
            .history(None, 100)
            .expect("metadata history")
            .iter()
            .filter(|h| h.action == "delete")
            .count(),
        2
    );
    assert!(vault.read_path(&WorkspacePath::parse("rules/a.md").expect("path")).is_err());
}
