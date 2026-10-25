use super::*;

#[tokio::test]
async fn s06_d4_live_authority_is_not_writable_through_file_approval() {
    let f = Fixture::new(PermissionMode::FullAccess).await;
    let database = f.service().persistence.database_path();
    let batch = f.admit(&[
        (
            "fs.write",
            json!({"path":database,"text":"forbidden",
            "expected":{"expected_kind":"absent"}}),
        ),
        (
            "fs.write",
            json!({"path":"aura.db","text":"ordinary fixture",
            "expected":{"expected_kind":"absent"}}),
        ),
    ]);
    let engine = f.service().agent_execution_required();
    let decisions = batch
        .batch
        .calls
        .iter()
        .map(|value| {
            let call = &batch.calls[&value.tool_call_id];
            let backend = engine
                .broker_backend(call)
                .expect("a permitted file call has its real broker backend")
                .descriptor();
            aura_permission_engine::policy::authorize(
                &call.policy(UtcTimestamp::now(), engine.runtime),
                &backend,
                None,
            )
        })
        .collect::<Vec<_>>();
    assert!(
        matches!(decisions[1], PolicyAuthorization::Allow { .. }),
        "ordinary project files with the same name stay writable"
    );
    assert!(
        matches!(decisions[0], PolicyAuthorization::Deny { .. }),
        "the actual authority database cannot receive an ordinary file approval: {:?}",
        decisions[0]
    );
    let blocked = batch.batch.calls[0].tool_call_id;
    let ordinary = batch.batch.calls[1].tool_call_id;
    f.execute(batch).await;
    f.output(ordinary);
    assert_eq!(
        std::fs::read(f.root.join("aura.db")).expect("read the actual file effect after dispatch"),
        b"ordinary fixture"
    );
    let store =
        f.service().broker.lock().expect("lock the existing authority store for inspection");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), blocked)
        .expect("read the durable tool call at this dispatch boundary");
    assert!(call.attempt_id.is_none(), "authority mutation is refused before dispatch");
}

#[tokio::test]
async fn s06_d3_verified_walk_excludes_live_authority_and_reports_skipped_ranges() {
    let f = Fixture::with_exposure(PermissionMode::FullAccess, &["fs.find", "fs.list"]).await;
    // The fixture's own folder: its release manifest is a live control file, and Aura's data
    // folders (with the databases) are beside it. A
    // walk from outside Aura's data does not go in, so the databases are not found either.
    let directory = f._host.root.path();
    let batch = f.admit(&[
        ("fs.find", json!({"path":directory,"file_names":["manifest.json","aura.db","aura-v2.db"],
            "recursive":true,"match_mode":"exact","case_sensitive":false,"limit":100,"cursor":null})),
        ("fs.list", json!({"path":directory,"limit":100,"cursor":null})),
    ]);
    let ids = batch.batch.calls.iter().map(|call| call.tool_call_id).collect::<Vec<_>>();
    f.execute(batch).await;
    let found = f.output(ids[0]);
    assert_eq!(
        found["matches"],
        json!([]),
        "recursive name lookup leaked a live authority resource"
    );
    assert_eq!(
        found["complete"], false,
        "excluded scopes must not look like a complete negative result"
    );
    assert!(
        !found["skipped_scopes"]
            .as_array()
            .expect("the real tool result contains the expected collection")
            .is_empty()
    );
    let listed = f.output(ids[1]);
    assert!(
        listed["entries"]
            .as_array()
            .expect("the real tool result contains the expected collection")
            .iter()
            .all(|entry| !matches!(entry["name"].as_str(), Some("manifest.json" | "local")))
    );
    assert!(
        !listed["skipped_scopes"]
            .as_array()
            .expect("the real tool result contains the expected collection")
            .is_empty()
    );
}

#[tokio::test]
async fn s06_d3_external_search_skips_live_authority_before_reading_and_reports_partial() {
    let f = Fixture::with_exposure(PermissionMode::FullAccess, &["fs.search"]).await;
    // The fixture's own folder, outside the workspace: its release manifest is a live control
    // file, and beside it are Aura's data folders. Searching Aura's data folder itself now asks first, so the search starts one level up.
    let directory = f._host.root.path();
    std::fs::write(directory.join("ordinary.txt"), "s06-visible-marker")
        .expect("write the fixture content for this permission boundary");
    let data = f
        .service()
        .persistence
        .database_path()
        .parent()
        .expect("the verified fixture path has its parent directory")
        .to_path_buf();
    std::fs::write(data.join("in-aura-data.txt"), "s06-visible-marker")
        .expect("the same text inside Aura's data");
    let batch = f.admit(&[(
        "fs.search",
        json!({"root":directory,
        "pattern":{"kind":"literal","text":"s06-visible-marker","case_sensitive":true},
        "path_glob":null,"include_ignored":false,"include_generated":false,"limit":100}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute(batch).await;
    let output = f.output(id);
    assert_eq!(
        output["matches"]
            .as_array()
            .expect("the real tool result contains the expected collection")
            .len(),
        1,
        "ordinary content remains searchable, and nothing in Aura's data is read: {output}"
    );
    assert_eq!(
        output["completion"]["status"], "partial",
        "an excluded scope cannot be complete: {output}"
    );
    assert!(
        output["completion"]["reasons"]
            .as_array()
            .expect("the real tool result contains the expected collection")
            .iter()
            .any(|r| r == "scope_excluded")
    );
    let skipped = output["skipped_scopes"]
        .as_array()
        .expect("the real tool result contains the expected collection")
        .iter()
        .map(|scope| {
            (
                scope["canonical_path"]
                    .as_str()
                    .expect("the scope projection contains a canonical path string")
                    .to_owned(),
                scope["reason"].as_str().unwrap_or_default().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert!(
        skipped
            .iter()
            .any(|(path, reason)| path.ends_with("manifest.json") && reason == "control_authority"),
        "{skipped:?}"
    );
    assert!(
        skipped.iter().any(|(path, reason)| path.ends_with("local") && reason == "protected_scope"),
        "a walk from outside Aura's data does not go in: {skipped:?}"
    );
}
