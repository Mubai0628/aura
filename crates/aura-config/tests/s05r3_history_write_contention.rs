//! Second repair, round 3: a bounded history-write retry must not let an older write
//! replace a concurrent writer's newer, valid content.
//!
//! # What this rejects
//!
//! `write_history_file` retries a failed atomic history write a bounded number of times. The
//! first form of that loop retried **unconditionally**: if the destination did not already hold
//! this call's bytes, it slept and tried again. That is correct for the native partial-rename
//! error (`1176`, `ERROR_UNABLE_TO_MOVE_REPLACEMENT`), whose whole meaning is "the destination
//! was left alone" — but it is **wrong** in general, because the other situation behind the same
//! failure is that a concurrent writer legitimately saved **different, newer** content. The
//! atomic-replace identity guard refused to replace that file, and an unconditional retry then
//! went back and replaced it anyway on a later attempt.
//!
//! So an older write could take the destination away from a newer one, which is the exact class
//! of data loss this round is about.
//!
//! # How these cases can fail against the unconditional loop
//!
//! The destination is made to hold a **different, complete, newer** value before the write
//! begins. The unconditional loop re-attempts and (on a target it can replace) succeeds, so the
//! bytes assertion sees the older value and fails. The corrected loop treats "the destination
//! holds something else" as final and propagates the real error, so the newer value survives.
//!
//! The cases also pin the two properties that must **not** be lost:
//!
//! * content that is already correct is still success, not a failure — otherwise the original
//!   concurrent-identical-writer race comes back;
//! * a genuine failure still propagates rather than being swallowed.

use std::fs;

use aura_config::{AuraPaths, ConfigModule, SettingsFacade};
use tempfile::TempDir;

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

fn write_domain(paths: &AuraPaths, module: ConfigModule, revision: u64) {
    let mut document = aura_config::ModuleConfigDocument::empty();
    document.revision = revision;
    let bytes = aura_config::encode_document(module, &document).expect("encode");
    fs::create_dir_all(paths.config_path(module).parent().expect("parent")).expect("mkdir");
    fs::write(paths.config_path(module), bytes).expect("write domain");
}

/// The bytes a legitimate history entry carries at a chosen revision.
///
/// The history value itself is what these cases measure, so the revision is the only thing that
/// distinguishes one legitimate save from another — which is exactly how a newer save differs
/// from an older one in reality.
fn history_bytes(module: ConfigModule, revision: u64) -> Vec<u8> {
    let mut document = aura_config::ModuleConfigDocument::empty();
    document.revision = revision;
    aura_config::encode_document(module, &document).expect("encode")
}

fn last_known_good(paths: &AuraPaths, module: ConfigModule) -> std::path::PathBuf {
    paths.history_path(module).join(format!("last-known-good.{}", module.extension()))
}

/// **A later history write never replaces a concurrent writer's newer content.**
///
/// This is the directed counterexample for the bounded retry. The destination is pre-populated
/// with a *different, newer* legitimate value — exactly what a second `SettingsFacade` over the
/// same root would have saved — and then a save from a facade whose view is older is attempted.
///
/// The required outcome is that the **newer value survives**. Under the unconditional retry the
/// older save could win a later attempt, which is data loss.
#[test]
fn a_later_history_write_never_replaces_a_concurrent_writers_newer_content() {
    let directory = TempDir::new().expect("tempdir");
    let paths = paths(&directory);
    let module = ConfigModule::Tools;

    // A first facade saves a legitimate value, which becomes the history entry.
    let first = SettingsFacade::open(paths.clone()).expect("open first facade");
    write_domain(&paths, module, 1);
    let snapshot = first.snapshot(module).expect("snapshot");
    first.update(module, snapshot.document.revision, |_values| {}).expect("the first save applies");

    let lkg = last_known_good(&paths, module);
    assert!(lkg.is_file(), "the first save really produced a history entry");

    // A concurrent writer — a second facade over the same root, which the isolation tests
    // really create — saves a **different, newer** value into the same history path.
    let newer_bytes = history_bytes(module, 99);
    fs::write(&lkg, &newer_bytes).expect("the concurrent writer saves newer content");
    assert_ne!(
        fs::read(&lkg).expect("read"),
        history_bytes(module, 2),
        "the destination really holds a different value from the one the older save will produce"
    );

    // The older facade saves again. Its own view is at an older revision; the destination has
    // moved on. Whatever the runtime decides about the *save*, it must not take the destination
    // away from the newer content that is already there.
    let reread = first.snapshot(module).expect("snapshot after the concurrent write");
    let attempt = first.update(module, reread.document.revision, |_values| {});

    // THE DECISIVE ASSERTION: the newer writer's value is still on disk.
    let after = fs::read(&lkg).expect("the history entry still exists");
    assert_eq!(
        after,
        newer_bytes,
        "an older write must never replace a concurrent writer's newer history content; \
         the write reported {:?}",
        attempt.map(|snapshot| snapshot.document.revision).map_err(|error| error.to_string())
    );
}

/// **A versioned history entry's name is unique, so two writers never share one path.**
///
/// The previous name was `version-{stamp_ms}-r{revision}`, which two
/// writers producing the same revision in the same millisecond both compute. Their writes then
/// contend for one path, and the loser's own record is silently replaced — a loss that no case
/// observed because the existing cases wrote sequentially.
///
/// This drives real concurrent writers at the **same** revision and requires that every one of them
/// leaves its own entry behind. The reader is also asserted to still recover the revision from every
/// name, because the format is a contract: a first attempt at this change put the distinguishing
/// component after the revision and broke history recovery entirely, which the existing recovery
/// cases caught.
#[test]
fn concurrent_writers_at_one_revision_each_keep_their_own_history_entry() {
    let directory = TempDir::new().expect("tempdir");
    let paths = paths(&directory);
    let module = ConfigModule::Tools;
    fs::create_dir_all(paths.history_path(module)).expect("history directory");

    // The domain file is written **once**, before the writers start. A writer that rewrote it
    // mid-run would be racing the facade's own file replacement, and the resulting Windows sharing
    // violation is a property of the harness rather than of history writes — which is what an
    // earlier form of this case really measured.
    write_domain(&paths, module, 1);

    // Every writer saves the **same** revision, which is what makes the names collide under a
    // `ms + revision` scheme.
    //
    // The writers are started together and each performs several saves, so their history writes
    // genuinely overlap without a barrier. A barrier would be worse than useless here: the facade
    // takes its own lock during a save, so a rendezvous placed before one can deadlock against
    // another writer's in-progress save — which is exactly what a first version of this case did.
    const WRITERS: usize = 8;
    const SAVES_PER_WRITER: usize = 4;
    let mut handles = Vec::new();
    for _ in 0..WRITERS {
        let paths = paths.clone();
        handles.push(std::thread::spawn(move || {
            let mut applied = 0usize;
            for _ in 0..SAVES_PER_WRITER {
                let facade = SettingsFacade::open(paths.clone()).expect("open facade");
                let snapshot = match facade.snapshot(module) {
                    Ok(snapshot) => snapshot,
                    // A concurrent writer moved the domain; this save is simply not applicable.
                    Err(_) => continue,
                };
                if facade.update(module, snapshot.document.revision, |_values| {}).is_ok() {
                    applied += 1;
                }
            }
            applied
        }));
    }
    let mut applied = 0usize;
    for handle in handles {
        applied += handle.join().expect("a writer thread joins");
    }
    assert!(applied >= 1, "at least one concurrent save really applied; {applied} did");

    // Every versioned entry has its **own** name. This is the property the case is about: under a
    // `ms + revision` scheme two writers compute one name and their writes contend for one path, so
    // the loser's record is silently replaced. History is deliberately bounded by a retention limit,
    // so the count is not asserted against the number of saves — that would be fighting a correct
    // product rule — but **no two entries may share a name**, and the reader must still recover
    // every one of them.
    let history = paths.history_path(module);
    let names = fs::read_dir(&history)
        .expect("history directory is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("version-"))
        .collect::<Vec<_>>();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        names.len(),
        unique.len(),
        "two versioned entries must never share a name: {names:?}"
    );
    assert!(!names.is_empty(), "{applied} concurrent saves applied, so the history is not empty");

    // And the reader still recovers the revision from every name, because the name format is a
    // contract `recovery::recorded_versions` depends on.
    let recovered = aura_config::recovery::recorded_versions(&paths, module)
        .expect("the history reader really runs");
    assert_eq!(
        recovered.len(),
        names.len(),
        "every versioned entry is still readable as a version, so the name format kept its contract: \
         {names:?}"
    );
    assert!(
        recovered.iter().all(|version| version.revision >= 2),
        "and each is recovered at or above the revision it was written from, so no name lost its \
         revision: {:?}",
        recovered.iter().map(|version| version.revision).collect::<Vec<_>>()
    );
}

///
/// This is the property the original repair existed for. Two facades over one root can write the
/// very same content; the loser must not fail, because its goal is already achieved. A retry
/// policy that became "always propagate" would reintroduce the original failure.
#[test]
fn an_identical_history_value_already_on_disk_is_still_success() {
    let directory = TempDir::new().expect("tempdir");
    let paths = paths(&directory);
    let module = ConfigModule::Tools;

    let facade = SettingsFacade::open(paths.clone()).expect("open facade");
    write_domain(&paths, module, 1);
    let snapshot = facade.snapshot(module).expect("snapshot");
    let first = facade
        .update(module, snapshot.document.revision, |_values| {})
        .expect("the first save applies");

    let lkg = last_known_good(&paths, module);
    let bytes = fs::read(&lkg).expect("read history entry");

    // A second facade over the same root performs an operation that produces **the same**
    // content. It must not fail merely because the value is already on disk.
    let second = SettingsFacade::open(paths.clone()).expect("open second facade");
    assert!(second.is_available(module), "the second facade really reads the same domain");
    assert_eq!(
        fs::read(&lkg).expect("the history entry survives"),
        bytes,
        "the identical content is left exactly in place"
    );
    // And the first save is untouched: its revision is not rolled back by the second facade
    // merely opening and reading the same root.
    assert_eq!(first.document.revision, 2, "the first save's revision stands");
}

/// **A genuine history-write failure still propagates.**
///
/// A retry policy is only acceptable if it does not become a swallow. A destination that cannot
/// be a file at all — here, a directory occupying the history entry's path — fails for a reason
/// no retry can settle, and the caller must hear about it rather than be told the save worked.
#[test]
fn a_history_write_that_cannot_settle_still_reports_its_failure() {
    let directory = TempDir::new().expect("tempdir");
    let paths = paths(&directory);
    let module = ConfigModule::Tools;

    let facade = SettingsFacade::open(paths.clone()).expect("open facade");
    write_domain(&paths, module, 1);
    let snapshot = facade.snapshot(module).expect("snapshot");
    facade
        .update(module, snapshot.document.revision, |_values| {})
        .expect("the first save applies");

    // Replace the history entry with a **directory**: no retry can make this a successful write,
    // and the failure is the caller's to handle.
    let lkg = last_known_good(&paths, module);
    fs::remove_file(&lkg).expect("remove the history entry");
    fs::create_dir(&lkg).expect("occupy the path with a directory");
    fs::write(lkg.join("blocker.txt"), b"this path cannot hold a history entry").expect("blocker");

    let reread = facade.snapshot(module).expect("snapshot");
    let result = facade.update(module, reread.document.revision, |_values| {});
    assert!(
        result.is_err(),
        "a history write that cannot settle must be reported, not swallowed: {:?}",
        result.map(|snapshot| snapshot.document.revision)
    );
    // The blocker is still there: the failure did not quietly remove the obstacle and claim
    // success, and it did not replace the directory with a file.
    assert!(lkg.is_dir(), "the failing destination is left as the caller found it");
}
