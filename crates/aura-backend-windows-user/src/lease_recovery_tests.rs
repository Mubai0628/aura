#![allow(clippy::unwrap_used)]
use super::*;
use aura_windows_platform::{ExecutionFileSecurity, RootJob};
use std::os::windows::fs::OpenOptionsExt;
const INSTALL: &str = "b3000000-0000-4000-8000-000000000002";
const ACCOUNT: &str = "S-1-5-21-33333-44444-55555-66666";
const WRITE: &str = "S-1-5-21-333333-444444-555555-666666";

struct Fixture {
    _temp: tempfile::TempDir,
    root: std::path::PathBuf,
    workspace: std::path::PathBuf,
    journal: std::path::PathBuf,
    scratch: std::path::PathBuf,
    owner: String,
    nonce: String,
    before: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join(INSTALL);
        let owner = aura_windows_platform::current_user_token_identity().unwrap().0;
        for relative in ["private", "private/execution-leases", "execution-scratch"] {
            let path = root.join(relative);
            std::fs::create_dir_all(&path).unwrap();
            let security = ExecutionFileSecurity::open(&path, false).unwrap();
            let before = security.capture().unwrap();
            let flags = if before.split('(').next().unwrap().contains("AI") { "PAI" } else { "P" };
            security
                .apply_exact(
                    &before,
                    &format!("D:{flags}(A;OICI;FA;;;{owner})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"),
                )
                .unwrap_or_else(|error| {
                    panic!("fixture ACL: {error}; before={before}; actual={:?}", security.capture())
                });
        }
        for name in ["execution.lock", "execution-setup.lock"] {
            std::fs::write(root.join("private").join(name), []).unwrap();
        }
        let mut random = [0; 16];
        getrandom::fill(&mut random).unwrap();
        let nonce = random.iter().map(|v| format!("{v:02x}")).collect::<String>();
        let journal = root.join(format!("private/execution-leases/{nonce}.jsonl"));
        let scratch = root.join("execution-scratch").join(&nonce);
        std::fs::create_dir(&scratch).unwrap();
        let workspace = temp.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let before = ExecutionFileSecurity::open(&workspace, false).unwrap().capture().unwrap();
        Self { _temp: temp, root, workspace, journal, scratch, owner, nonce, before }
    }
    fn lease(&self, guarded: bool) -> file_lease::FileLease {
        let mut lease = file_lease::FileLease::create(&self.journal).unwrap();
        if guarded {
            lease
                .bind_process(
                    INSTALL,
                    &self.nonce,
                    &aura_windows_platform::execution_maintenance::boot_identity().unwrap(),
                )
                .unwrap();
        }
        for root in [&self.scratch, &self.workspace] {
            let object = ExecutionFileSecurity::open(root, false).unwrap();
            let before = object.capture().unwrap();
            let expected = object.plan_for_attempt(ACCOUNT, Some(WRITE)).unwrap();
            drop(object);
            if let Err(error) = lease
                .grant_tree(
                    &file_binding::capture(root, false).unwrap().0,
                    ACCOUNT,
                    Some(WRITE),
                    &crate::file_lease::GrantControl::unbounded(),
                )
                .finish()
            {
                lease.defer_recovery();
                drop(lease);
                let actual = ExecutionFileSecurity::open(root, false).unwrap().capture();
                panic!(
                    "grant fixture {:?}: {error}; before={before}; expected={expected}; actual={actual:?}",
                    root.file_name()
                );
            }
        }
        lease
    }
    fn acl(&self) -> String {
        ExecutionFileSecurity::open(&self.workspace, false).unwrap().capture().unwrap()
    }
}

#[test]
fn overlapping_runtime_leases_recover_in_reverse_order_and_preserve_new_files() {
    for restored_layers in 0..=2 {
        let fixture = Fixture::new();
        let mut first = file_lease::FileLease::create(&fixture.journal).unwrap();
        let boot = aura_windows_platform::execution_maintenance::boot_identity().unwrap();
        first.bind_runtime_process(INSTALL, &fixture.nonce, &boot, 1).unwrap();
        first
            .grant_tree(
                &file_binding::capture(&fixture.scratch, false).unwrap().0,
                ACCOUNT,
                Some(WRITE),
                &crate::file_lease::GrantControl::unbounded(),
            )
            .finish()
            .unwrap();
        let root = file_binding::capture(&fixture.workspace, false).unwrap().0;
        first
            .grant_tree(&root, ACCOUNT, Some(WRITE), &crate::file_lease::GrantControl::unbounded())
            .finish()
            .unwrap();
        let nonce = if fixture.nonce != "ffffffffffffffffffffffffffffffff" {
            "ffffffffffffffffffffffffffffffff"
        } else {
            "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"
        };
        let mut second = file_lease::FileLease::create(
            &fixture.root.join(format!("private/execution-leases/{nonce}.jsonl")),
        )
        .unwrap();
        second.bind_runtime_process(INSTALL, nonce, &boot, 2).unwrap();
        let second_write = "S-1-5-21-333333-444444-555555-777777";
        second.share_roots_from(&first);
        second
            .grant_tree(
                &root,
                ACCOUNT,
                Some(second_write),
                &crate::file_lease::GrantControl::unbounded(),
            )
            .finish()
            .unwrap();
        std::fs::write(fixture.workspace.join("retained.txt"), "produced by the running tools")
            .unwrap();
        if restored_layers >= 1 {
            second.restore_existing().unwrap();
        }
        if restored_layers >= 2 {
            first.restore_existing().unwrap();
        }
        drop(second);
        drop(first);
        lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
        assert!(ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before));
        let acl = ExecutionFileSecurity::open(&fixture.workspace.join("retained.txt"), false)
            .unwrap()
            .capture()
            .unwrap();
        assert!(!acl.contains(WRITE) && !acl.contains(second_write));
        assert_eq!(
            std::fs::read_to_string(fixture.workspace.join("retained.txt")).unwrap(),
            "produced by the running tools"
        );
    }
}

#[test]
fn recovery_restores_bound_lease_after_stop_without_replaying_or_removing_workspace_data() {
    let fixture = Fixture::new();
    let lease = fixture.lease(true);
    let job = RootJob::create_recoverable(INSTALL, &fixture.nonce).unwrap();
    std::fs::write(fixture.workspace.join("output.txt"), "already executed").unwrap();
    drop(lease);
    assert!(fixture.acl().contains(WRITE), "Drop must not restore a possibly active process lease");
    let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(report["command_replayed"], false);
    assert_eq!(job.active_processes().unwrap(), 0);
    assert!(ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before));
    assert!(!fixture.scratch.exists());
    assert_eq!(
        std::fs::read_to_string(fixture.workspace.join("output.txt")).unwrap(),
        "already executed"
    );
    lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
}

#[test]
fn recovery_refuses_busy_installation_before_touching_permissions() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    let lock = aura_windows_platform::lock_execution_resources(
        &fixture.root.join("private/execution-setup.lock"),
    )
    .unwrap();
    assert!(lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).is_err());
    assert!(fixture.acl().contains(WRITE));
    drop(lock);
    lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
}

#[test]
fn recovery_preserves_unproven_legacy_and_foreign_process_bindings() {
    for legacy in [true, false] {
        let fixture = Fixture::new();
        let mut lease = fixture.lease(!legacy);
        lease.defer_recovery();
        drop(lease);
        let before = std::fs::read(&fixture.journal).unwrap();
        let id = if legacy { INSTALL } else { "b3000000-0000-4000-8000-000000000003" };
        assert!(lease_recovery::recover_idle(&fixture.root, id, &fixture.owner).is_err());
        assert_eq!(std::fs::read(&fixture.journal).unwrap(), before);
        assert!(fixture.acl().contains(WRITE));
        // Isolated fixture cleanup uses its exact local journal only.
        file_lease::recover(&fixture.journal).unwrap();
    }
}

#[test]
fn recovery_rejects_malformed_boot_before_restoring_any_acl() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    let text = std::fs::read_to_string(&fixture.journal).unwrap();
    let (first, remaining) = text.split_once('\n').unwrap();
    let mut header: serde_json::Value = serde_json::from_str(first).unwrap();
    header["boot"] = serde_json::json!("------------------------------------");
    let changed = format!("{}\n{remaining}", serde_json::to_string(&header).unwrap());
    std::fs::write(&fixture.journal, &changed).unwrap();
    assert!(lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).is_err());
    assert!(fixture.acl().contains(WRITE));
    assert_eq!(std::fs::read_to_string(&fixture.journal).unwrap(), changed);
    file_lease::recover(&fixture.journal).unwrap();
}

// ---------------------------------------------------------------------------
// The trusted released index: historical journals are read once, not on every
// open, and any doubt about the cached entry falls back to the full read.
// ---------------------------------------------------------------------------

#[test]
fn released_journals_are_indexed_once_and_not_re_read() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    let cold = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    // One read to learn the journal is unreleased, one to capture the final
    // content after the restore appended its Released record.
    assert_eq!(cold["work"]["journals"], 1);
    assert_eq!(cold["work"]["parsed"], 2);
    assert_eq!(cold["work"]["indexed"], 0);
    assert!(cold["work"]["bytes"].as_u64().unwrap() > 0);
    assert!(cold["work"]["records"].as_u64().unwrap() > 0);
    assert!(ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before));

    let hot = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(hot["work"]["journals"], 1);
    assert_eq!(hot["work"]["parsed"], 0);
    assert_eq!(hot["work"]["indexed"], 1);
    assert_eq!(hot["work"]["bytes"], 0);
    assert_eq!(hot["work"]["records"], 0);
    assert!(ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before));
}

#[test]
fn a_replaced_released_journal_is_re_read_and_reindexed() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    // Swap in different released content under the same name: the replacement
    // has a new file identity, so the index entry must not speak for it.
    let replacement =
        serde_json::to_string(&serde_json::json!({"entry":"released"})).unwrap() + "\n";
    std::fs::remove_file(&fixture.journal).unwrap();
    std::fs::write(&fixture.journal, &replacement).unwrap();
    let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(report["work"]["parsed"], 1);
    assert_eq!(report["work"]["indexed"], 0);
    let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(report["work"]["indexed"], 1);
    assert_eq!(report["work"]["parsed"], 0);
}

#[test]
fn a_released_journal_replaced_by_an_unreleased_one_takes_the_full_path() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    let first = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(first["work"]["indexed"], 0);
    // An unreleased journal bound to this installation, boot and file name,
    // with a scope and no intents: it must be restored and settled by the real
    // recovery path, never answered from the stale index entry.
    let root = file_binding::capture(&fixture.workspace, false).unwrap().0;
    let stem = fixture.journal.file_stem().unwrap().to_str().unwrap().to_owned();
    let boot = aura_windows_platform::execution_maintenance::boot_identity().unwrap();
    let boundary = serde_json::json!({"entry":"process_boundary","installation":INSTALL,"nonce":stem,"boot":boot,"order":1});
    let scope = serde_json::json!({"entry":"scope","root":serde_json::to_value(&root).unwrap(),"account":ACCOUNT,"write_sid":null});
    std::fs::remove_file(&fixture.journal).unwrap();
    std::fs::write(
        &fixture.journal,
        format!(
            "{}\n{}\n",
            serde_json::to_string(&boundary).unwrap(),
            serde_json::to_string(&scope).unwrap()
        ),
    )
    .unwrap();
    let second = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    // Two reads: one to learn the replacement is unreleased, one to capture the
    // final content after the restore appended its Released record.
    assert_eq!(second["work"]["parsed"], 2, "a replaced journal is re-read in full");
    assert_eq!(second["work"]["indexed"], 0);
    assert!(ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before));
    // The replacement settled (no intents) and is itself indexed now.
    let third = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(third["work"]["indexed"], 1);
    assert_eq!(third["work"]["parsed"], 0);
}

#[test]
fn a_corrupt_or_foreign_index_is_ignored_and_rebuilt() {
    for foreign in [false, true] {
        let fixture = Fixture::new();
        drop(fixture.lease(true));
        lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
        let index = fixture.root.join("private/execution-leases-index.json");
        if foreign {
            let value = serde_json::json!({"schema":"windows-user-released-lease-index-v1","installation":"b3000000-0000-4000-8000-000000000099","journals":{}});
            std::fs::write(&index, serde_json::to_vec(&value).unwrap()).unwrap();
        } else {
            std::fs::write(&index, b"not json").unwrap();
        }
        let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
        assert_eq!(report["work"]["parsed"], 1, "a doubtful index is simply no index");
        assert_eq!(report["work"]["indexed"], 0);
        let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
        assert_eq!(report["work"]["indexed"], 1);
    }
}

#[test]
fn a_large_released_history_is_indexed_once_and_not_re_read() {
    let fixture = Fixture::new();
    // This test never opens the fixture lease, so its empty per-attempt
    // scratch root has no journal and must not trip the preservation guard.
    std::fs::remove_dir(&fixture.scratch).unwrap();
    // One hundred settled journals from earlier runs of this installation.
    for i in 0..100_u32 {
        let nonce = format!("{i:032x}");
        std::fs::write(
            fixture.root.join(format!("private/execution-leases/{nonce}.jsonl")),
            "{\"entry\":\"released\"}\n",
        )
        .unwrap();
    }
    let cold = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(cold["work"]["journals"], 100);
    assert_eq!(cold["work"]["parsed"], 100, "the first open reads the whole history once");
    assert_eq!(cold["work"]["indexed"], 0);
    let hot = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(hot["work"]["journals"], 100);
    assert_eq!(hot["work"]["parsed"], 0, "a warm open never re-reads settled history");
    assert_eq!(hot["work"]["indexed"], 100);
    assert_eq!(hot["work"]["bytes"], 0);
    assert_eq!(hot["work"]["records"], 0);
    assert_eq!(
        hot["work"]["witnessed"], 100,
        "the warm open really checked every entry's change generation"
    );
}

// ---------------------------------------------------------------------------
// The released index is only believed while the journal still *is* the file the
// entry was built from. Identity and size are not that proof: a rewrite can keep
// both. These cases are the ones the v5 counterexample was built from.
// ---------------------------------------------------------------------------

/// Put the workspace back into the state a live lease leaves behind, so a pass
/// that answers from a stale cache is visible: it would report success while the
/// recorded grant stays on disk.
fn regrant_workspace(fixture: &Fixture) {
    let object = ExecutionFileSecurity::open(&fixture.workspace, false).unwrap();
    let before = object.capture().unwrap();
    let after = object.plan_for_attempt(ACCOUNT, Some(WRITE)).unwrap();
    object.apply_exact(&before, &after).unwrap();
}

/// Valid, unreleased journal content of exactly the original byte length: the
/// settlement records are dropped and the final line is padded, so the file keeps
/// its name, its identity and its size while every byte that described the
/// settlement is gone.
fn unreleased_same_length(original: &str) -> String {
    let kept = original
        .lines()
        .filter(|line| !line.contains("\"objects_restored\"") && !line.contains("\"released\""))
        .collect::<Vec<_>>();
    let mut rebuilt = format!("{}\n", kept.join("\n"));
    assert!(
        rebuilt.len() <= original.len(),
        "the dropped settlement records must be longer than the padding budget"
    );
    let padding = original.len() - rebuilt.len();
    let insert_at = rebuilt.len() - 1;
    rebuilt.insert_str(insert_at, &" ".repeat(padding));
    assert_eq!(rebuilt.len(), original.len(), "the rewrite must keep the exact size");
    assert_ne!(rebuilt, original);
    rebuilt
}

#[test]
fn a_released_journal_rewritten_to_the_same_length_is_read_and_restored() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    let cold = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(cold["work"]["indexed"], 0);
    let hot = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(hot["work"]["indexed"], 1, "a quiet released journal is a legitimate hot hit");

    // The counterexample: same object, same size, different bytes, and the
    // authorization state the fixture recorded back on disk.
    regrant_workspace(&fixture);
    let original = std::fs::read_to_string(&fixture.journal).unwrap();
    let before_identity = file_binding::capture(&fixture.journal, false).unwrap().0;
    std::fs::write(&fixture.journal, unreleased_same_length(&original)).unwrap();
    let after_identity = file_binding::capture(&fixture.journal, false).unwrap().0;
    assert_eq!(before_identity.file_id, after_identity.file_id, "the object is the same");
    assert_eq!(
        before_identity.sha256, None,
        "the identity check does not carry a hash; the bytes really changed"
    );

    let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert!(
        report["work"]["parsed"].as_u64().unwrap() >= 1,
        "a same-length rewrite must not be answered from the index: {report}"
    );
    assert!(report["work"]["bytes"].as_u64().unwrap() > 0, "the content was read");
    assert!(
        ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before),
        "the grant the rewrite described is really restored: {}",
        fixture.acl()
    );
    assert!(
        std::fs::read_to_string(&fixture.journal).unwrap().contains("\"entry\":\"released\""),
        "the rewritten journal settled on its real content"
    );
}

#[test]
fn a_released_journal_a_writer_still_holds_is_never_served_from_the_index() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    let hot = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(hot["work"]["indexed"], 1);

    regrant_workspace(&fixture);
    let original = std::fs::read_to_string(&fixture.journal).unwrap();
    let rebuilt = unreleased_same_length(&original);
    // A write handle that stays open: the volume's generation advances once for
    // the whole session, so the number alone would still match the index.
    let mut writer =
        std::fs::OpenOptions::new().write(true).share_mode(7).open(&fixture.journal).unwrap();
    {
        use std::io::{Seek, SeekFrom, Write};
        writer.write_all(&rebuilt.as_bytes()[..rebuilt.len() / 2]).unwrap();
        writer.flush().unwrap();
        writer.seek(SeekFrom::Start(0)).unwrap();
        writer.write_all(rebuilt.as_bytes()).unwrap();
        writer.flush().unwrap();
    }
    let held = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner);
    assert!(held.is_err(), "a journal that cannot be witnessed is not settled: {held:?}");
    assert!(
        !ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before),
        "nothing was reported as restored while the journal could not be read"
    );
    assert!(!std::fs::read_to_string(&fixture.journal).unwrap().contains("\"released\""));

    drop(writer);
    let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert!(
        report["work"]["parsed"].as_u64().unwrap() >= 1,
        "the content was read once it could be"
    );
    assert!(ExecutionFileSecurity::sddl_equivalent(&fixture.acl(), &fixture.before));
    assert!(std::fs::read_to_string(&fixture.journal).unwrap().contains("\"entry\":\"released\""));
}

#[test]
fn an_index_without_a_change_generation_is_no_index() {
    let fixture = Fixture::new();
    drop(fixture.lease(true));
    lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    let hot = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(hot["work"]["indexed"], 1);

    // The format before this proof existed: identity, size and a hash, with no
    // way to show the file still matches. It must not be trusted.
    let index = fixture.root.join("private/execution-leases-index.json");
    let mut wire: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&index).unwrap()).unwrap();
    let journals = wire["journals"].as_object_mut().unwrap();
    for (_name, entry) in journals.iter_mut() {
        entry.as_object_mut().unwrap().remove("change_usn");
    }
    wire.as_object_mut().unwrap().remove("epoch");
    std::fs::write(&index, serde_json::to_vec(&wire).unwrap()).unwrap();

    let report = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(report["work"]["parsed"], 1, "an index that cannot prove anything is re-read");
    assert_eq!(report["work"]["indexed"], 0);
    let reindexed = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    assert_eq!(reindexed["work"]["indexed"], 1, "the rebuilt index carries the proof again");
    assert_eq!(reindexed["work"]["parsed"], 0);
}

// ---------------------------------------------------------------------------
// Measurement harness (ignored by default): the real released history, copied
// into an isolated installation root, cold then warm through the production
// entry point. Set AURA_LEASE_PERF_CORPUS to the directory that holds the
// installed journals; the originals are only read.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "measurement harness: needs AURA_LEASE_PERF_CORPUS"]
fn release_cache_cold_and_warm_on_a_real_history_copy() {
    let Ok(corpus) = std::env::var("AURA_LEASE_PERF_CORPUS") else {
        println!("AURA_LEASE_PERF_CORPUS is not set: no corpus to measure");
        return;
    };
    let corpus = std::path::PathBuf::from(corpus);
    let fixture = Fixture::new();
    std::fs::remove_dir(&fixture.scratch).unwrap();
    let mut copied = 0_u64;
    let mut bytes = 0_u64;
    let mut skipped = 0_u64;
    for entry in std::fs::read_dir(&corpus).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().is_none_or(|value| value != "jsonl") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        if !text.lines().last().is_some_and(|line| line.contains("\"entry\":\"released\"")) {
            skipped += 1;
            continue;
        }
        copied += 1;
        bytes += std::fs::metadata(&path).unwrap().len();
        std::fs::copy(&path, fixture.root.join("private/execution-leases").join(entry.file_name()))
            .unwrap();
    }
    println!("copied {copied} released journals, {bytes} bytes; skipped {skipped} unsettled");
    let cold_started = std::time::Instant::now();
    let cold = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    let cold_ms = cold_started.elapsed().as_millis();
    println!("cold {cold_ms} ms: {}", cold["work"]);
    let warm_started = std::time::Instant::now();
    let warm = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    let warm_ms = warm_started.elapsed().as_millis();
    println!("warm {warm_ms} ms: {}", warm["work"]);
    let again_started = std::time::Instant::now();
    let again = lease_recovery::recover_idle(&fixture.root, INSTALL, &fixture.owner).unwrap();
    println!("warm again {} ms: {}", again_started.elapsed().as_millis(), again["work"]);
    assert_eq!(cold["work"]["indexed"], 0);
    assert_eq!(warm["work"]["parsed"], 0);
    assert_eq!(warm["work"]["bytes"], 0);
    assert_eq!(warm["work"]["indexed"], copied);
    assert_eq!(warm["work"]["witnessed"], copied);
}
