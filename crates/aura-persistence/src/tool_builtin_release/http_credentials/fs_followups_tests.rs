//! Non-secret filesystem configuration survives the real upgrade from revision 6 to the current
//! release: revision 7, and 8 since fs.write and fs.patch return the logical hash.
use super::*;
use crate::{PersistenceOptions, PersistenceStartup};
use aura_domain_common::RuntimeInstanceId;
use aura_tool_registry::agent_release::AgentBuiltinEntry;

#[test]
fn s10_write_and_patch_revision7_carry_configuration_without_crossing_tool_identity() {
    let root = tempfile::tempdir().expect("isolated filesystem configuration store");
    let store = match PersistenceCoordinator::start(PersistenceOptions::new(
        root.path().join("aura.db"),
        root.path().join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("start store")
    {
        PersistenceStartup::Ready(store) => store,
        PersistenceStartup::RecoveryRequired(recovery) => panic!("{}", recovery.reason),
    };
    // Exact historical Rust-generated entries from 014dfe2; do not reconstruct old schemas.
    let mut old: Vec<AgentBuiltinEntry> = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/tools/s10-followups-revision6.json"
    )))
    .expect("published revision 6 filesystem entries");
    assert_eq!(old.len(), 2);
    for (index, entry) in old.iter_mut().enumerate() {
        entry.definition.validate().expect("historical definition fingerprint");
        let published = aura_tool_registry::agent_release::revisions::PUBLISHED
            .iter()
            .find(|r| r.name == entry.handler.name() && r.revision == 6)
            .expect("immutable revision 6");
        assert_eq!(entry.definition.definition_fingerprint.hex(), published.fingerprint);
        assert_eq!(entry.instance.tool_instance_id.to_string(), published.instance);
        entry.instance.enabled = index == 1;
        entry.instance.overrides.timeout_ms = Some(60_000 + index as u64 * 1_000);
        entry.instance.overrides.max_retries = Some(index as u8);
    }
    let seed = old.clone();
    store
        .dispatch(move |db| {
            let tx = db.transaction()?;
            for entry in seed {
                insert_definition(&tx, &entry.definition)?;
                insert_instance(&tx, &entry.instance)?;
            }
            tx.commit()?;
            Ok(())
        })
        .expect("saved revision 6 user settings");
    let release = aura_tool_registry::agent_release::release().expect("verified current release");
    let current = old
        .iter()
        .map(|entry| {
            let new = release.entry(entry.handler.name()).expect("same filesystem Tool").clone();
            assert_eq!(new.definition.tool_id, entry.definition.tool_id);
            let current =
                aura_tool_registry::agent_release::revisions::current(entry.handler.name())
                    .expect("published filesystem Tool")
                    .revision;
            assert!(current > 6, "the release moved past the saved revision 6");
            assert_eq!(new.definition.tool_version.definition_revision, Revision::new(current));
            assert_ne!(new.instance.tool_instance_id, entry.instance.tool_instance_id);
            new
        })
        .collect::<Vec<_>>();
    store
        .materialize_agent_builtin_configuration(current.clone(), UtcTimestamp::now())
        .expect("real configuration upgrade")
        .expect("both new instances materialized");
    let authority = store.tool_authority_snapshot(None, None, None).expect("upgraded authority");
    for (old, new) in old.iter().zip(&current) {
        let saved = authority
            .instances
            .iter()
            .find(|i| i.tool_instance_id == new.instance.tool_instance_id)
            .expect("new instance configuration");
        assert_eq!(saved.enabled, old.instance.enabled);
        assert_eq!(saved.overrides, old.instance.overrides);
        assert!(
            authority.instances.iter().any(|i| i.tool_instance_id == old.instance.tool_instance_id)
        );
    }
    let revision = store.with_reader(read_registry_revision).expect("upgrade registry revision");
    assert!(
        store
            .materialize_agent_builtin_configuration(current, UtcTimestamp::now())
            .expect("idempotent repeat")
            .is_none()
    );
    assert_eq!(store.with_reader(read_registry_revision).expect("same revision"), revision);
}
