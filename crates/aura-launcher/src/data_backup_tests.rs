use super::*;

const V2: &str = "data/v2-execution/aura-v2.db";
const LEGACY: &str = "data/aura.db";

fn stores(data: &Path, text: &str) {
    for store in [LEGACY, V2, "data/v2-execution/aura-v2.db-wal"] {
        let path = data.join(store);
        fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        fs::write(path, format!("{text} {store}")).expect("store");
    }
}

fn read(data: &Path, store: &str) -> String {
    fs::read_to_string(data.join(store)).expect("store")
}

fn plenty(_: &Path) -> io::Result<u64> {
    Ok(u64::MAX)
}

#[test]
fn a_copy_taken_before_a_version_puts_the_stores_back_and_keeps_what_it_replaced() {
    let data = tempfile::tempdir().expect("data");
    stores(data.path(), "old");
    let taken = take_with(data.path(), "0.1.1", None, plenty).expect("take");
    assert!(matches!(taken, Taken::Copied { stores: 3, .. }), "{taken:?}");
    stores(data.path(), "migrated");
    assert!(restore(data.path(), "0.1.1").expect("restore"));
    assert_eq!(read(data.path(), LEGACY), "old data/aura.db");
    assert_eq!(
        read(data.path(), "data/v2-execution/aura-v2.db-wal"),
        "old data/v2-execution/aura-v2.db-wal"
    );
    let kept: Vec<_> = fs::read_dir(data.path().join("backups"))
        .expect("backups")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("replaced-"))
        .collect();
    assert_eq!(kept.len(), 1);
    let replaced = data.path().join("backups").join(&kept[0]);
    assert_eq!(read(&replaced, LEGACY), "migrated data/aura.db");
}

#[test]
fn only_the_latest_copy_stays_and_a_version_without_one_restores_nothing() {
    let data = tempfile::tempdir().expect("data");
    stores(data.path(), "first");
    take_with(data.path(), "0.1.1", None, plenty).expect("take");
    take_with(data.path(), "0.1.2", None, plenty).expect("take again");
    assert!(!backup_folder(data.path(), "0.1.1").exists());
    assert!(backup_folder(data.path(), "0.1.2").is_dir());
    assert!(!restore(data.path(), "0.1.1").expect("nothing to restore"));
    assert_eq!(read(data.path(), LEGACY), "first data/aura.db");
}

#[test]
fn an_update_that_migrates_nothing_copies_nothing_and_drops_older_copies() {
    let data = tempfile::tempdir().expect("data");
    stores(data.path(), "first");
    take_with(data.path(), "0.1.1", None, plenty).expect("an older copy");
    let none = BTreeSet::new();
    assert_eq!(
        take_with(data.path(), "0.1.2", Some(&none), plenty).expect("skip"),
        Taken::NothingToCopy
    );
    assert!(!backup_folder(data.path(), "0.1.2").exists());
    assert!(!backup_folder(data.path(), "0.1.1").exists(), "it could only put back older data");
}

#[test]
fn only_the_store_to_migrate_is_copied_and_a_restore_leaves_the_other_alone() {
    let data = tempfile::tempdir().expect("data");
    stores(data.path(), "old");
    let v2_only = BTreeSet::from([V2]);
    let taken = take_with(data.path(), "0.1.1", Some(&v2_only), plenty).expect("take");
    assert!(matches!(taken, Taken::Copied { stores: 2, .. }), "{taken:?}");
    let folder = backup_folder(data.path(), "0.1.1");
    assert!(folder.join(V2).is_file());
    assert!(!folder.join(LEGACY).exists());
    stores(data.path(), "new");
    assert!(restore(data.path(), "0.1.1").expect("restore"));
    assert_eq!(read(data.path(), V2), "old data/v2-execution/aura-v2.db");
    assert_eq!(read(data.path(), LEGACY), "new data/aura.db", "a store the copy lacks stays");
}

#[test]
fn a_disk_without_room_for_the_copy_skips_it() {
    let data = tempfile::tempdir().expect("data");
    stores(data.path(), "old");
    let taken = take_with(data.path(), "0.1.1", None, |_| Ok(1)).expect("skip");
    assert!(matches!(taken, Taken::NoRoom { free: 1, needed } if needed > 1), "{taken:?}");
    assert!(!backup_folder(data.path(), "0.1.1").exists());
}

#[test]
fn only_a_plain_no_leaves_a_store_out() {
    let all = parse_pending(r#"{"data/aura.db":true,"data/v2-execution/aura-v2.db":true}"#);
    assert_eq!(all, Some(BTreeSet::from([LEGACY, V2])));
    let none = parse_pending(r#"{"data/aura.db":false,"data/v2-execution/aura-v2.db":false}"#);
    assert_eq!(none, Some(BTreeSet::new()));
    let failed = parse_pending(
        r#"{"data/aura.db":{"error":"locked"},"data/v2-execution/aura-v2.db":false}"#,
    );
    assert_eq!(failed, Some(BTreeSet::from([LEGACY])), "an error copies the store");
    assert_eq!(parse_pending(r#"{}"#), Some(BTreeSet::from([LEGACY, V2])));
    assert_eq!(parse_pending("not json"), None);
}

#[test]
fn a_core_that_cannot_answer_copies_every_store() {
    let data = tempfile::tempdir().expect("data");
    assert_eq!(pending_stores(&data.path().join("missing-core.exe"), data.path(), "0.1.0"), None);
}

#[test]
fn a_restore_request_is_read_once() {
    let data = tempfile::tempdir().expect("data");
    let path = data.path().join(RESTORE_FILE);
    fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    fs::write(&path, r#"{"before_version":"0.1.1"}"#).expect("request");
    assert_eq!(take_request(data.path()), Some(RestoreRequest { before_version: "0.1.1".into() }));
    assert_eq!(take_request(data.path()), None);
}

fn at(text: &str) -> UtcTimestamp {
    text.parse().expect("timestamp")
}

fn backup_names(data: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(data.join("backups"))
        .expect("backups")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn a_replaced_copy_goes_thirty_days_after_it_was_set_aside_and_nothing_else_does() {
    let data = tempfile::tempdir().expect("data");
    let names = [
        "before-0.2.0",
        "replaced-2026-10-01T00-00-00Z",
        "replaced-2026-10-29T18-08-43-5616171Z",
        "replaced-unreadable",
    ];
    for name in names {
        let folder = data.path().join("backups").join(name).join("data");
        fs::create_dir_all(&folder).expect("folder");
        fs::write(folder.join("aura.db"), name).expect("store");
    }
    // Just under 30 days after the newer copy was set aside: only the older copy goes.
    let first = remove_expired_copies(data.path(), at("2026-11-28T18:08:43Z"));
    assert_eq!(first.removed, ["replaced-2026-10-01T00-00-00Z"]);
    assert!(first.failed.is_empty());
    let second = remove_expired_copies(data.path(), at("2026-11-28T18:08:44Z"));
    assert_eq!(second.removed, ["replaced-2026-10-29T18-08-43-5616171Z"]);
    assert_eq!(backup_names(data.path()), ["before-0.2.0", "replaced-unreadable"]);
    let third = remove_expired_copies(data.path(), at("2027-06-01T00:00:00Z"));
    assert!(third.removed.is_empty(), "a name without a time and a before- copy stay");
}

#[test]
fn the_time_a_restore_writes_into_the_name_is_the_time_the_expiry_reads() {
    let data = tempfile::tempdir().expect("data");
    stores(data.path(), "old");
    take_with(data.path(), "0.1.1", None, plenty).expect("take");
    let before = UtcTimestamp::now();
    assert!(restore(data.path(), "0.1.1").expect("restore"));
    let name = backup_names(data.path())
        .into_iter()
        .find(|name| name.starts_with("replaced-"))
        .expect("the replaced copy");
    let thirty_days = |days: i64| {
        UtcTimestamp::from_value(
            before.value() + std::time::Duration::from_secs(days as u64 * 86_400),
        )
    };
    assert!(remove_expired_copies(data.path(), thirty_days(29)).removed.is_empty());
    assert_eq!(remove_expired_copies(data.path(), thirty_days(31)).removed, [name]);
}
