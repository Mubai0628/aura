use super::consent_fixture::approve_file;
use super::*;

#[tokio::test]
async fn s06_b2_own_publication_follows_the_verified_new_object_but_replacement_does_not() {
    let f = Fixture::new(PermissionMode::ReadOnly).await;
    let batch = f.admit(&[(
        "fs.write",
        json!({"path":"approved.txt","text":"first","expected":{"expected_kind":"absent"}}),
    )]);
    let call = batch.calls.values().next().expect("new target must be admitted");
    let grant = approve_file(&f, call);
    let id = call.validated.call.tool_call_id;
    f.execute(batch).await;
    let diagnostics = {
        let store =
            f.service().broker.lock().expect("lock the existing authority store for inspection");
        let mut query = store.database.connection().prepare(
            "SELECT payload FROM events WHERE kind='tool.dispatch_adapter_error' ORDER BY sequence",
        ).expect("prepare the dispatch diagnostic query");
        query
            .query_map([], |row| row.get::<_, String>(0))
            .expect("read the recorded dispatch adapter diagnostics")
            .collect::<Result<Vec<_>, _>>()
            .expect("decode every dispatch diagnostic row")
    };
    assert!(diagnostics.is_empty(), "dispatch refused before a file effect: {diagnostics:?}");
    f.output(id);
    let grants = aura_persistence_v2::file_consents::list(
        f.service()
            .broker
            .lock()
            .expect("lock the existing authority store for inspection")
            .database
            .connection(),
        f.turn.session_id,
    )
    .expect("read the existing session file approvals");
    let updated = grants
        .iter()
        .find(|g| g.grant_id == grant.grant_id)
        .expect("the approved grant remains present in the session store");
    let target = &updated
        .scope
        .file_consent
        .as_ref()
        .expect("the approved grant contains its file scope")
        .targets[0];
    let info = aura_windows_platform::inspect_existing_path(&f.root.join("approved.txt"), true)
        .expect("verify the native identity of the fixture target");
    assert_eq!(
        target.identity_key,
        aura_security_contract::file_object_key(&aura_workspace::domain_identity(&info.identity)),
        "own handle-bound publication follows the approved target"
    );
    std::fs::rename(f.root.join("approved.txt"), f.root.join("moved-away.txt"))
        .expect("replace the fixture entry from outside Aura");
    std::fs::write(f.root.join("approved.txt"), "other object")
        .expect("write the fixture content for this permission boundary");
    let replacement =
        aura_windows_platform::inspect_existing_path(&f.root.join("approved.txt"), true)
            .expect("verify the native identity of the fixture target");
    assert_ne!(
        target.identity_key,
        aura_security_contract::file_object_key(&aura_workspace::domain_identity(
            &replacement.identity
        )),
        "an unrelated replacement does not inherit permission"
    );
}

#[tokio::test]
async fn s06_b2_staged_overwrite_receipt_keeps_approval_on_its_own_published_object() {
    let f = Fixture::with_exposure_setup(PermissionMode::ReadOnly, &["fs.write"], |root| {
        std::fs::write(root.join("overwrite.txt"), "before")
            .expect("write the fixture content for this permission boundary");
    })
    .await;
    let batch = f.admit(&[(
        "fs.write",
        json!({
            "path":"overwrite.txt", "text":"own overwrite",
            "expected":{"expected_kind":"file_hash","hash":ContentHash::blake3(b"before")},
        }),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    let consent = approve_file(&f, &batch.calls[&id]);
    f.execute(batch).await;
    f.output(id);
    assert_eq!(
        std::fs::read(f.root.join("overwrite.txt"))
            .expect("read the actual file effect after dispatch"),
        b"own overwrite"
    );
    let published =
        aura_windows_platform::inspect_existing_path(&f.root.join("overwrite.txt"), true)
            .expect("verify the native identity of the fixture target");
    let grants = aura_persistence_v2::file_consents::list(
        f.service()
            .broker
            .lock()
            .expect("lock the existing authority store for inspection")
            .database
            .connection(),
        f.turn.session_id,
    )
    .expect("read the existing session file approvals");
    let grant = grants
        .iter()
        .find(|grant| grant.grant_id == consent.grant_id)
        .expect("the approved grant remains present in the session store");
    assert_eq!(
        grant
            .scope
            .file_consent
            .as_ref()
            .expect("the approved grant contains its file scope")
            .targets[0]
            .identity_key,
        aura_security_contract::file_object_key(&aura_workspace::domain_identity(
            &published.identity
        )),
        "the staged publication still carries its verified original object in the receipt"
    );
}
