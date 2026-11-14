//! A model catalog version replaces the one before it: only the current one stays, with its raw
//! response, so the catalog no longer grows by a version each time the upstream list changes. The
//! run before a start drops the versions an older version kept.
use std::collections::BTreeMap;

use aura_artifact_contract::{
    ArtifactAccess, ArtifactOwnerRef, ArtifactScope, ArtifactSystemOwner,
};
use aura_domain_common::{ArtifactId, ContentHash, Revision, UtcTimestamp};
use aura_persistence::{ArtifactWriteRequest, ProviderCatalogCacheWrite};

use super::*;

/// A system-cache artifact past the inline limit, so its bytes are object files on disk.
fn system_artifact(coordinator: &PersistenceCoordinator, fill: u8) -> (ArtifactId, Vec<u8>) {
    let bytes = vec![fill; 40 * 1024];
    let record = coordinator
        .write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::SystemCache,
            media_type: "application/json".to_owned(),
            access: ArtifactAccess::USER_READ,
            owner: ArtifactOwnerRef::System { owner: ArtifactSystemOwner::ProviderCatalogCache },
            bytes: bytes.clone(),
        })
        .unwrap();
    (record.artifact_id, bytes)
}

fn catalog_version(coordinator: &PersistenceCoordinator, revision: u64) -> ArtifactId {
    let (raw_artifact_id, bytes) = system_artifact(coordinator, revision as u8);
    let now = UtcTimestamp::now();
    coordinator
        .store_provider_catalog_snapshot(ProviderCatalogCacheWrite {
            catalog_revision: Revision::new(revision),
            catalog_content_hash: ContentHash::blake3(&bytes),
            checked_at: now,
            last_successful_refresh_at: now,
            etag: None,
            last_modified: None,
            raw_artifact_id,
            snapshot_json: serde_json::json!({ "revision": revision }),
            entries_json: BTreeMap::from([
                ("deepseek".to_owned(), serde_json::json!({ "revision": revision })),
                ("openai".to_owned(), serde_json::json!({ "revision": revision })),
            ]),
        })
        .unwrap();
    raw_artifact_id
}

fn artifact_exists(db: &Connection, id: ArtifactId) -> bool {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM artifacts WHERE artifact_id=?1)",
        [id.to_string()],
        |row| row.get(0),
    )
    .unwrap()
}

fn object_files(store: &Store) -> usize {
    fn walk(path: &Path) -> usize {
        std::fs::read_dir(path)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| if entry.file_type().unwrap().is_dir() { walk(&entry.path()) } else { 1 })
            .sum()
    }
    walk(&store.artifacts)
}

#[test]
fn a_new_catalog_version_replaces_the_one_before_it_with_its_raw_response() {
    let store = Store::new();
    let coordinator = store.start();
    let (icon, _) = system_artifact(&coordinator, 0xee);
    let first = catalog_version(&coordinator, 1);
    let second = catalog_version(&coordinator, 2);
    assert_eq!(object_files(&store), 2, "the second version's raw response replaced the first's");
    let third = catalog_version(&coordinator, 3);

    let db = store.raw();
    assert_eq!(count(&db, "SELECT count(*) FROM provider_catalog_snapshots"), 1);
    assert_eq!(count(&db, "SELECT count(*) FROM provider_catalog_entries"), 2);
    assert_eq!(
        count(&db, "SELECT count(*) FROM provider_catalog_entries WHERE catalog_revision='3'"),
        2
    );
    assert!(!artifact_exists(&db, first) && !artifact_exists(&db, second));
    assert!(artifact_exists(&db, third) && artifact_exists(&db, icon), "icons share the owner");
    assert_eq!(object_files(&store), 2, "the old raw responses left the disk");
    let cache = coordinator.load_provider_catalog_cache().unwrap();
    assert_eq!(cache.snapshot.unwrap().catalog_revision, Revision::new(3));
    // The same version stored again is still one version, with one raw response.
    let artifacts = count(&db, "SELECT count(*) FROM artifacts");
    let again = catalog_version(&coordinator, 3);
    assert_eq!(count(&db, "SELECT count(*) FROM provider_catalog_snapshots"), 1);
    assert_eq!(count(&db, "SELECT count(*) FROM artifacts"), artifacts);
    assert!(artifact_exists(&db, again) && !artifact_exists(&db, third));
    assert_eq!(object_files(&store), 2);
}

#[test]
fn the_run_drops_the_versions_an_older_version_kept() {
    let store = Store::new();
    let coordinator = store.start();
    let current = catalog_version(&coordinator, 9);
    // An older version kept every version it stored.
    let older: Vec<_> = (7..9_u64)
        .map(|revision| {
            let (raw, bytes) = system_artifact(&coordinator, revision as u8);
            (revision, raw, ContentHash::blake3(&bytes))
        })
        .collect();
    drop(coordinator);
    let db = store.raw();
    for (revision, raw, hash) in &older {
        db.execute(
            "INSERT INTO provider_catalog_snapshots(catalog_revision, catalog_content_hash,
                checked_at, last_successful_refresh_at, raw_artifact_id, snapshot_json, created_at)
             VALUES (?1, ?2, '2026-10-01T00:00:00Z', '2026-10-01T00:00:00Z', ?3, '{}',
                '2026-10-01T00:00:00Z')",
            rusqlite::params![
                revision.to_string(),
                serde_json::to_string(hash).unwrap(),
                raw.to_string()
            ],
        )
        .unwrap();
        db.execute(
            "INSERT INTO provider_catalog_entries(catalog_revision, catalog_provider_id, entry_json)
             VALUES (?1, 'openai', '{}')",
            [revision.to_string()],
        )
        .unwrap();
    }
    assert_eq!(object_files(&store), 3);
    // The run has the file alone, as before a start.
    drop(db);

    let report = store.slim(u64::MAX / 4);
    assert_eq!(report.catalog_versions, 2);
    let db = store.raw();
    assert_eq!(count(&db, "SELECT count(*) FROM provider_catalog_snapshots"), 1);
    assert_eq!(
        count(&db, "SELECT count(*) FROM provider_catalog_entries WHERE catalog_revision!='9'"),
        0
    );
    assert!(older.iter().all(|(_, raw, _)| !artifact_exists(&db, *raw)));
    assert!(artifact_exists(&db, current));
    assert_eq!(object_files(&store), 1);
    drop(db);
    let reopened = store.start();
    let cache = reopened.load_provider_catalog_cache().unwrap();
    assert_eq!(cache.snapshot.unwrap().catalog_revision, Revision::new(9));
}
