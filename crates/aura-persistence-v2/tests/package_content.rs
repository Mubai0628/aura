//! Real SQLite artifacts/ownership, never legacy tables or credentials.
use aura_domain_common::{ToolPackageId, UtcTimestamp};
use aura_persistence_v2::{
    V2Store,
    packages::{self, PackageFileBytes},
};
use aura_tool_domain::PackageContentDescriptorV1;

const RUNTIME: &[u8] = b"stdio fixture runtime";

fn descriptor() -> PackageContentDescriptorV1 {
    // This fixture is produced by the real authoritative stager/fingerprint code.
    let fixture = aura_contract_schema::canonical_fixture().expect("actual package evidence").tools;
    PackageContentDescriptorV1 {
        manifest: fixture.canonical_package_manifest.expect("manifest"),
        package_fingerprint: fixture.package_fingerprint.expect("fingerprint"),
        security_fingerprint: fixture.package_security_fingerprint.expect("security fingerprint"),
        runtime_closure_hash: fixture.package_runtime_closure_hash.expect("closure"),
    }
}

fn count(db: &rusqlite::Connection, table: &str) -> i64 {
    db.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
        .expect("fixture count")
}

#[test]
fn repeated_import_deduplicates_payload_and_artifacts_without_installing_or_trusting() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("v2");
    let mut store = V2Store::create(&root).expect("store");
    let descriptor = descriptor();
    let files = [PackageFileBytes { path: &descriptor.manifest.files[0].path, bytes: RUNTIME }];
    let first = store
        .database
        .unit_of_work(|tx| {
            packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &files,
                UtcTimestamp::now(),
            )
        })
        .expect("first import");
    let second = store
        .database
        .unit_of_work(|tx| {
            packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &files,
                UtcTimestamp::now(),
            )
        })
        .expect("same import");
    assert_eq!(first, second, "same identity and revision, not another version");
    let db = store.database.connection();
    assert_eq!(count(db, "tool_packages"), 1);
    assert_eq!(count(db, "tool_package_versions"), 1);
    assert_eq!(count(db, "tool_package_payloads"), 1);
    assert_eq!(count(db, "tool_package_references"), 1);
    assert_eq!(count(db, "artifact_objects"), 2, "one canonical manifest and one payload file");
    assert_eq!(count(db, "artifacts"), 2, "repeat import does not make duplicate artifact handles");
    assert_eq!(count(db, "tool_package_trust"), 0);
    assert_eq!(count(db, "events"), 2, "each import has an immutable audit event");
    let state = packages::state(db, first.package_id).expect("state").expect("package");
    assert!(!state.installed && !state.enabled && !state.trusted);
    let refs: i64 = db
        .query_row("SELECT ref_count FROM tool_package_payloads", [], |row| row.get(0))
        .expect("references");
    assert_eq!(refs, 1);
    drop(store);
    let reopened = V2Store::open(&root).expect("restart");
    let loaded =
        packages::load_content(reopened.database.connection(), &first).expect("durable bytes");
    assert_eq!(loaded.descriptor, descriptor);
    assert_eq!(loaded.files[0].1, RUNTIME);
}

#[test]
fn bad_file_bytes_roll_back_content_references_and_audit_together() {
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("store");
    let descriptor = descriptor();
    let files = [PackageFileBytes {
        path: &descriptor.manifest.files[0].path,
        bytes: b"changed fixture bytes",
    }];
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &files,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    for table in [
        "tool_packages",
        "tool_package_versions",
        "tool_package_payloads",
        "tool_package_files",
        "tool_package_references",
        "artifact_objects",
        "artifacts",
        "events",
    ] {
        assert_eq!(count(store.database.connection(), table), 0, "failed import left {table}");
    }
}

#[test]
fn damaged_artifact_cannot_be_read_or_reused_as_verified_content() {
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("store");
    let descriptor = descriptor();
    let files = [PackageFileBytes { path: &descriptor.manifest.files[0].path, bytes: RUNTIME }];
    let binding = store
        .database
        .unit_of_work(|tx| {
            packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &files,
                UtcTimestamp::now(),
            )
        })
        .expect("import");
    store
        .database
        .connection()
        .execute("UPDATE artifact_objects SET bytes=zeroblob(byte_length)", [])
        .expect("corrupt temporary fixture");
    assert!(packages::load_content(store.database.connection(), &binding).is_err());
    assert!(
        store
            .database
            .unit_of_work(|tx| packages::import_content(
                tx,
                ToolPackageId::new(),
                &descriptor,
                &files,
                UtcTimestamp::now()
            ))
            .is_err()
    );
    assert_eq!(
        count(store.database.connection(), "events"),
        1,
        "no audit success after failed reuse"
    );
    assert_eq!(count(store.database.connection(), "tool_package_references"), 1);
}
