use super::*;

fn canonical<T: serde::Serialize>(value: &T) -> String {
    String::from_utf8(
        aura_domain_common::jcs::canonical_json_jcs(value).expect("canonical fixture"),
    )
    .expect("UTF-8 fixture")
}

#[tokio::test]
async fn s09_a_saved_builtin_disable_refuses_an_old_snapshot_without_an_attempt() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let batch = f.admit(&[("http.request", json!({"url":server.url}))]);
    let id = batch.batch.calls[0].tool_call_id;
    assert_eq!(batch.calls.len(), 1, "the old snapshot really admitted an enabled tool");
    let entry = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry("http.request")
        .expect("HTTP");
    let mut instance = f
        .service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("authority")
        .instances
        .into_iter()
        .find(|i| i.tool_instance_id == entry.instance.tool_instance_id)
        .expect("saved HTTP owner");
    instance.enabled = false;
    instance.entity_revision = Revision::new(instance.entity_revision.value() + 1);
    instance.updated_at = UtcTimestamp::now();
    let db = rusqlite::Connection::open(f.service().persistence.database_path())
        .expect("isolated saved configuration");
    db.execute("UPDATE tool_instances SET enabled=0,entity_revision=?1,record_json=?2,record_hash_json=?3,updated_at=?4 WHERE tool_instance_id=?5",rusqlite::params![i64::try_from(instance.entity_revision.value()).expect("revision"),canonical(&instance),canonical(&aura_domain_common::jcs::canonical_sha256(&instance).expect("instance hash")),instance.updated_at.to_string(),instance.tool_instance_id.to_string()]).expect("closed saved builtin");
    db.execute(
        "UPDATE tool_registry_authority_state SET registry_revision=registry_revision+1",
        [],
    )
    .expect("invalidate the saved authority cache");
    drop(db);
    f.service()
        .persistence
        .tool_authority_snapshot(None, None, None)
        .expect("valid saved authority");
    f.execute_rejecting(batch).await;
    no_attempt(&f, id);
    assert_eq!(server.count(), 0, "a disabled tool must never start its HTTP request");
    let store = f.service().broker.lock().expect("paired refusal");
    let call =
        aura_persistence_v2::tool::execution::call(store.database.connection(), id).expect("call");
    assert!(matches!(
        call.result.expect("refusal result").outcome,
        ToolResultOutcomeV1::NotExecuted { .. }
    ));
}
