//! Real authenticated Main routes, TEMP Vaults, actual configuration CAS and file repair.
use super::*;
use aura_memory::*;
#[tokio::test]
async fn a_repair_committed_before_a_metadata_failure_is_done_and_keeps_its_history() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (_, w, _) = workspace(&fixture, &state).await;
    let memory = services(&state).broker.memory();
    let resolver = services(&state)
        .p03()
        .expect("L05 desktop fixture")
        .workspace
        .resolver(w.workspace_id)
        .expect("L05 desktop fixture");
    let ws =
        MemoryWorkspace { workspace_id: w.workspace_id, root: resolver.canonical_root().into() };
    let vault = memory.vault(MemoryScope::Project, &ws).expect("L05 desktop fixture");
    vault
        .write(
            &[MemoryWrite {
                path: aura_domain_common::WorkspacePath::parse("rules/test.md")
                    .expect("L05 desktop fixture"),
                note: MemoryDocument {
                    metadata: MemoryMetadata::new(MemoryScope::Project, "rules".into()),
                    body: "# 原正文\n".into(),
                },
                expected_hash: None,
            }],
            &WriteActor::default(),
        )
        .expect("L05 desktop fixture");
    let path = vault.root().join("rules/test.md");
    let text = std::fs::read_to_string(&path).expect("L05 desktop fixture");
    let broken =
        text.lines().filter(|l| !l.starts_with("confidence:")).collect::<Vec<_>>().join("\n")
            + "\n";
    std::fs::write(&path, &broken).expect("L05 desktop fixture");
    let input = json!({"scope":"project","workspace_id":w.workspace_id});
    let preview = success(&state, "memory.vault.repair.preview", input.clone()).await;
    let records = std::fs::read_dir(&memory.roots().cache)
        .expect("L05 desktop fixture")
        .map(|entry| entry.expect("L05 desktop fixture").path())
        .find(|path| {
            path.is_dir() && path.extension().is_some_and(|extension| extension == "records")
        })
        .expect("L05 desktop fixture");
    // A TEMP-only obstruction: the file publication can complete, but recording
    // its metadata cannot. No ACL, registry or user configuration is changed.
    assert!(records.starts_with(fixture.root.path()));
    std::fs::rename(records.join("history"), records.join("saved-history"))
        .expect("L05 desktop fixture");
    std::fs::write(records.join("history"), b"TEMP obstruction").expect("L05 desktop fixture");
    let response = request(
        &state,
        "memory.vault.repair.apply",
        json!({"vault":input,"preview_id":preview["preview_token"]}),
    )
    .await;
    assert_ne!(
        std::fs::read_to_string(&path).expect("L05 desktop fixture"),
        broken,
        "repair really published before the metadata failure"
    );
    assert!(std::fs::read_to_string(&path).expect("L05 desktop fixture").contains("# 原正文"));
    // The batch committed before its history could be recorded: the repair is done,
    // and the committed journal keeps the history to repeat at the next open.
    assert!(response["error"].is_null(), "a committed repair is reported as done: {response}");
    let manifests = std::fs::read_dir(records.join("pending"))
        .expect("L05 desktop fixture")
        .map(|entry| entry.expect("L05 desktop fixture").path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
        .map(|path| {
            serde_json::from_slice::<Value>(&std::fs::read(path).expect("L05 desktop fixture"))
                .expect("L05 desktop fixture")
        })
        .collect::<Vec<_>>();
    assert_eq!(manifests.len(), 1);
    assert_eq!(manifests[0]["committed"], true);
    assert_eq!(manifests[0]["history"][0]["action"], "repair");
}
#[tokio::test]
async fn management_reads_do_not_create_vaults_and_settings_use_revision_cas() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (root, w, _) = workspace(&fixture, &state).await;
    let input = json!({"workspace_id":w.workspace_id});
    let view = success(&state, "memory.vaults.get", input).await;
    assert_eq!(view["vaults"].as_array().expect("L05 desktop fixture").len(), 2);
    assert!(
        view["vaults"]
            .as_array()
            .expect("L05 desktop fixture")
            .iter()
            .all(|v| v["exists"] == false)
    );
    assert!(!root.join(".memories").exists());
    assert!(!services(&state).broker.memory().roots().global.exists());
    let settings = success(&state, "memory.settings.get", json!({})).await;
    assert_eq!(settings["settings"]["automatic_maintenance"], true);
    let update = json!({"expected_revision":settings["revision"],"settings":{"automatic_maintenance":false,"event_retention_days":17}});
    let saved = success(&state, "memory.settings.update", update.clone()).await;
    assert_eq!(saved["settings"]["event_retention_days"], 17);
    let conflict = request(&state, "memory.settings.update", update).await;
    assert_eq!(conflict["error"]["data"]["code"], "memory.conflict");
    let invalid=request(&state,"memory.settings.update",json!({"expected_revision":saved["revision"],"settings":{"automatic_maintenance":true,"event_retention_days":0}})).await;
    assert!(invalid.get("error").is_some());
    assert_eq!(success(&state, "memory.settings.get", json!({})).await, saved);
    assert!(!root.join(".memories").exists());
}
#[tokio::test]
async fn validate_and_preview_preserve_files_apply_requires_bound_hashes() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let (_, w, _) = workspace(&fixture, &state).await;
    let memory = services(&state).broker.memory();
    let resolver = services(&state)
        .p03()
        .expect("L05 desktop fixture")
        .workspace
        .resolver(w.workspace_id)
        .expect("L05 desktop fixture");
    let ws =
        MemoryWorkspace { workspace_id: w.workspace_id, root: resolver.canonical_root().into() };
    let vault = memory.vault(MemoryScope::Project, &ws).expect("L05 desktop fixture");
    let note = MemoryDocument {
        metadata: MemoryMetadata::new(MemoryScope::Project, "rules".into()),
        body: "# 不改正文\n保留人工内容。\n".into(),
    };
    vault
        .write(
            &[MemoryWrite {
                path: aura_domain_common::WorkspacePath::parse("rules/note.md")
                    .expect("L05 desktop fixture"),
                note: note.clone(),
                expected_hash: None,
            }],
            &WriteActor::default(),
        )
        .expect("L05 desktop fixture");
    let path = vault.root().join("rules/note.md");
    let original = std::fs::read_to_string(&path).expect("L05 desktop fixture");
    let broken =
        original.lines().filter(|l| !l.starts_with("confidence:")).collect::<Vec<_>>().join("\n")
            + "\n";
    std::fs::write(&path, &broken).expect("L05 desktop fixture");
    let input = json!({"scope":"project","workspace_id":w.workspace_id});
    let report = success(&state, "memory.vault.validate", input.clone()).await;
    assert_eq!(report["invalid"].as_array().expect("L05 desktop fixture").len(), 1);
    let preview = success(&state, "memory.vault.repair.preview", input.clone()).await;
    assert!(!preview["changes"].as_array().expect("L05 desktop fixture").is_empty());
    assert_eq!(std::fs::read_to_string(&path).expect("L05 desktop fixture"), broken);
    std::fs::write(&path, format!("{broken}\nexternal edit\n")).expect("L05 desktop fixture");
    let refused = request(
        &state,
        "memory.vault.repair.apply",
        json!({"vault":input,"preview_id":preview["preview_token"]}),
    )
    .await;
    assert_eq!(refused["error"]["data"]["code"], "memory.conflict");
    let preview = success(&state, "memory.vault.repair.preview", input.clone()).await;
    let applied = success(
        &state,
        "memory.vault.repair.apply",
        json!({"vault":input,"preview_id":preview["preview_token"]}),
    )
    .await;
    assert_eq!(applied["applied"], true);
    let content = std::fs::read_to_string(&path).expect("L05 desktop fixture");
    assert!(content.contains(&note.body));
    assert!(content.contains("external edit"));
    let _ = success(&state, "memory.vault.rebuild", input.clone()).await;
    let handle = success(&state, "memory.export.prepare", input).await;
    assert!(handle["entries"].as_u64().expect("L05 desktop fixture") > 0);
    let _ = success(&state, "memory.export.close", json!({"export_id":handle["export_id"]})).await;
}
#[tokio::test]
async fn no_main_auth_or_caller_selected_root_can_borrow_management_authority() {
    let fixture = Fixture::new();
    let state = fixture.start().await;
    let response =
        request(&state, "memory.vaults.get", json!({"workspace_id":null,"root":"C:/not-a-vault"}))
            .await;
    assert!(response.get("error").is_some());
    assert!(
        aura_ipc::routes::registry()
            .all(|route| !(route.surface == aura_ipc::routes::RouteSurface::Main
                && route.method.starts_with("memory.export.")))
    );
    state.authenticated_channel.lock().expect("L05 desktop fixture").peer_handshake = None;
    let response = request(&state, "memory.settings.get", json!({})).await;
    assert_eq!(response["error"]["data"]["code"], "memory.not_authorized");
}
