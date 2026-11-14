use aura_domain_common::{SessionId, WorkspaceId};
use aura_memory::*;
use serde_json::json;

fn output(scope: &str, body: &str) -> MemoryMaintenanceOutput {
    serde_json::from_value(json!({"schema_version":1,"candidates":[{"action":"create","scope":scope,"path":"rules/learned.md","content":{"category":"rules","body":body,"basis":"本轮已完成并验证的工作"}}],"events":[{"title":"完成离线验证","summary":"本机临时目录中的测试通过。","basis":"工具结果"}]})).expect("scripted output")
}
#[test]
fn an_empty_valid_response_archives_expired_events_without_creating_empty_vaults() {
    use aura_domain_common::{UtcTimestamp, WorkspacePath};
    let temp = tempfile::tempdir().expect("TEMP");
    let workspace =
        MemoryWorkspace { workspace_id: WorkspaceId::new(), root: temp.path().join("workspace") };
    std::fs::create_dir(&workspace.root).expect("workspace");
    let service =
        MemoryService::new(VaultRoots::new(temp.path().join("global"), temp.path().join("cache")));
    let output = MemoryMaintenanceOutput { schema_version: 1, candidates: vec![], events: vec![] };
    let actor = WriteActor { kind: "automatic_maintenance".into(), ..Default::default() };
    service.apply_maintenance(&output, &workspace, &actor, "empty", 7, || true).expect("empty");
    assert!(!workspace.root.join(".memories").exists());
    let vault = service.vault(MemoryScope::Project, &workspace).expect("project");
    let old = UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::days(40));
    vault
        .append_events(
            &[ProjectEvent {
                id: blake3::hash(b"old").to_hex().to_string(),
                at: old,
                title: "已经验证的工作".into(),
                summary: "保留期缩短前保留在 recent".into(),
                session_id: None,
            }],
            365,
            &actor,
        )
        .expect("old recent");
    service
        .apply_maintenance(&output, &workspace, &actor, "retention", 7, || true)
        .expect("retention without new event");
    assert!(
        !vault
            .read_path(&WorkspacePath::parse("events/recent.md").expect("path"))
            .expect("recent")
            .note
            .body
            .contains("保留期缩短前")
    );
    let archive = WorkspacePath::parse(format!(
        "events/archive/{:04}-{:02}.md",
        old.value().year(),
        u8::from(old.value().month())
    ))
    .expect("archive");
    assert!(vault.read_path(&archive).expect("archived").note.body.contains("保留期缩短前"));
}
#[test]
fn schema_secret_and_final_permission_validation_happen_before_any_vault_write() {
    let temp = tempfile::tempdir().expect("TEMP");
    let workspace =
        MemoryWorkspace { workspace_id: WorkspaceId::new(), root: temp.path().join("workspace") };
    std::fs::create_dir(&workspace.root).expect("workspace");
    let global = temp.path().join("global");
    let service = MemoryService::new(VaultRoots::new(global.clone(), temp.path().join("cache")));
    let actor = WriteActor {
        kind: "automatic_maintenance".into(),
        session_id: Some(SessionId::new()),
        turn_id: Some("temporary-turn".into()),
    };
    let mut invalid = output("global", "# 合法规则\n");
    invalid.schema_version = 2;
    assert!(
        service
            .apply_maintenance(&invalid, &workspace, &actor, "run-fixture", 30, || true)
            .is_err()
    );
    let secret = output(
        "global",
        "# 不可保存\n-----BEGIN PRIVATE KEY-----\ntemporary-test-value\n-----END PRIVATE KEY-----\n",
    );
    assert!(matches!(
        service.apply_maintenance(&secret, &workspace, &actor, "run-fixture", 30, || true),
        Err(MemoryError::Secret { .. })
    ));
    let valid = output("global", "# 构建约定\n使用本机离线验证。\n");
    assert!(
        service.apply_maintenance(&valid, &workspace, &actor, "run-fixture", 30, || false).is_err()
    );
    assert!(!global.exists());
    assert!(!workspace.root.join(".memories").exists());
    let first = service
        .apply_maintenance(&valid, &workspace, &actor, "run-fixture", 30, || true)
        .expect("apply structured fixture");
    assert_eq!(first.created, 1);
    assert!(global.join("rules/learned.md").is_file());
    let second = service
        .apply_maintenance(&valid, &workspace, &actor, "run-fixture", 30, || true)
        .expect("deduplicate");
    assert_eq!(second.created, 0);
    assert_eq!(second.ignored, 1);
    let recent =
        std::fs::read_to_string(workspace.root.join(".memories/events/recent.md")).expect("events");
    assert_eq!(recent.matches("<!-- aura-event ").count(), 1);
    let vault = service.vault(MemoryScope::Global, &workspace).expect("global");
    assert!(
        vault
            .history(None, 100)
            .expect("history")
            .iter()
            .all(|h| h.actor == "automatic_maintenance")
    );
}

#[test]
fn an_invalid_second_scope_does_not_partially_publish_the_first_scope() {
    let temp = tempfile::tempdir().expect("TEMP");
    let work = temp.path().join("workspace");
    std::fs::create_dir(&work).expect("workspace");
    let workspace = MemoryWorkspace { workspace_id: WorkspaceId::new(), root: work };
    let service =
        MemoryService::new(VaultRoots::new(temp.path().join("global"), temp.path().join("cache")));
    let mut model = output("global", "# first\n");
    model.events.clear();
    let mut bad = model.candidates[0].clone();
    if let MemoryCandidate::Create { scope, path, .. } = &mut bad {
        *scope = MemoryScope::Project;
        *path = aura_domain_common::WorkspacePath::parse("README.md").expect("relative");
    }
    model.candidates.push(bad);
    assert!(
        service
            .apply_maintenance(&model, &workspace, &WriteActor::default(), "run", 30, || true)
            .is_err()
    );
    assert!(!temp.path().join("global").exists());
    assert!(!workspace.root.join(".memories").exists());
}
