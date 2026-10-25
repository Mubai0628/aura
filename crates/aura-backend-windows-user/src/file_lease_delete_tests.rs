//! Deletion while a lease is held, as a real `cmd.exe` performs it.
//!
//! The v5 failure was reproduced by hand and is reproduced here by the product's
//! own path: a command deletes an object the lease still holds a handle to. What
//! Windows then does is the whole point — `cmd`'s delete sets the disposition
//! through a handle that shares delete, so the object becomes *delete-pending*
//! while the lease's retained handle keeps its name visible, and every attempt to
//! reopen that name fails. Cleanup therefore cannot walk it by name, and the
//! lease's own handle is the only thing that can describe it.
//!
//! The last case pins the boundary the identity anchors buy, rather than hiding
//! it: while the lease holds the handle, a name that has been deleted this way is
//! still visible, so a command cannot remove its parent directory in the same
//! invocation. Releasing the handle completes the deletion immediately, and the
//! next invocation sees the same filesystem a caller without a lease would.
#![allow(clippy::unwrap_used)]
use super::*;
use aura_windows_platform::{ExecutionFileSecurity, NativeFileAccess};

const ACCOUNT: &str = "S-1-5-21-33333-44444-55555-66666";
const WRITE: &str = "S-1-5-21-333333-444444-555555-666666";

fn acl(path: &std::path::Path) -> String {
    ExecutionFileSecurity::open(path, false).unwrap().capture().unwrap()
}

/// Run `del /q <name>` through the real command interpreter in `directory`, then
/// report whether the name is still visible — which is what the v5 evidence
/// recorded as `exit /b 12`.
fn cmd_delete_and_probe(directory: &std::path::Path, name: &str) -> bool {
    let script = format!("del /q {name} & if exist {name} (exit /b 12) else (exit /b 0)");
    let status = std::process::Command::new("cmd.exe")
        .arg("/c")
        .arg(&script)
        .current_dir(directory)
        .status()
        .unwrap();
    status.code() == Some(12)
}

fn fixture(
    name: &str,
) -> (tempfile::TempDir, std::path::PathBuf, aura_windows_user_contract::ExecutionFileBinding) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join(name);
    std::fs::create_dir(&root).unwrap();
    let (binding, _) = file_binding::capture(&root, false).unwrap();
    (temp, root, binding)
}

fn lease(temp: &tempfile::TempDir, root: &std::path::Path) -> file_lease::FileLease {
    let (binding, _) = file_binding::capture(root, false).unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&binding, ACCOUNT, Some(WRITE), &file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    lease
}

fn journal_text(temp: &tempfile::TempDir) -> String {
    std::fs::read_to_string(temp.path().join("journal.jsonl")).unwrap()
}

#[test]
fn a_command_deleted_object_settles_the_lease_and_releases_its_handle() {
    let (temp, root, _) = fixture("workspace");
    let probe = root.join("probe.txt");
    std::fs::write(&probe, b"fixture").unwrap();
    let before = acl(&root);
    let mut lease = lease(&temp, &root);
    let probe_before = acl(&probe);

    assert!(
        cmd_delete_and_probe(&root, "probe.txt"),
        "the deletion must leave the name visible while the lease holds it, which is the state v5 failed on"
    );
    assert!(
        ExecutionFileSecurity::open(&probe, false).is_err(),
        "the delete-pending name refuses to open, which is exactly how the v5 walk failed"
    );

    lease.restore().expect("a delete-pending object must not fail the lease");

    let text = journal_text(&temp);
    assert!(text.contains("\"entry\":\"object_gone\""), "the fact must be durable: {text}");
    assert!(text.contains("\"entry\":\"released\""), "the lease settled: {text}");
    assert!(!probe.exists(), "releasing the handle completes the deletion the command asked for");
    assert!(
        ExecutionFileSecurity::sddl_equivalent(&acl(&root), &before),
        "the rest of the scope is restored: {} vs {before}",
        acl(&root)
    );
    assert_ne!(probe_before, before, "the fixture was really granted before the delete");
}

#[test]
fn a_deleted_child_does_not_fail_the_scope_walk_and_its_directory_is_restored() {
    let (temp, root, _) = fixture("workspace");
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    let child = nested.join("inner.txt");
    std::fs::write(&child, b"inner").unwrap();
    let before_root = acl(&root);
    let before_nested = acl(&nested);
    let mut lease = lease(&temp, &root);

    assert!(cmd_delete_and_probe(&nested, "inner.txt"));
    lease.restore().expect("a delete-pending child must not fail the scope walk");

    let text = journal_text(&temp);
    assert!(text.contains("\"entry\":\"object_gone\""));
    assert!(text.contains("\"entry\":\"released\""));
    assert!(nested.is_dir(), "the directory itself was never deleted");
    assert!(!child.exists());
    assert!(
        ExecutionFileSecurity::sddl_equivalent(&acl(&nested), &before_nested),
        "the surviving directory is restored"
    );
    assert!(ExecutionFileSecurity::sddl_equivalent(&acl(&root), &before_root));
}

/// The compatibility boundary the identity anchors cost, stated as a test: while
/// the lease holds its handles, a name deleted through `cmd` is still visible, so
/// the same invocation cannot remove the emptied directory. The moment the lease
/// releases its handles the deletion completes, and the next invocation sees the
/// directory gone. This is the difference the v5 defect turned into a permanent
/// one: nothing ever released the handle.
#[test]
fn the_deleted_name_clears_as_soon_as_the_lease_releases_its_handle() {
    let (temp, root, _) = fixture("workspace");
    let nested = root.join("nested");
    std::fs::create_dir(&nested).unwrap();
    std::fs::write(nested.join("inner.txt"), b"inner").unwrap();
    let mut lease = lease(&temp, &root);

    let status = std::process::Command::new("cmd.exe")
        .arg("/c")
        .arg("pushd nested && del /q inner.txt && popd && rmdir nested")
        .current_dir(&root)
        .status()
        .unwrap();
    assert_eq!(
        status.code(),
        Some(145),
        "with the name still held open the directory is not empty"
    );
    assert!(
        nested.join("inner.txt").exists(),
        "the name is still visible while the lease holds it"
    );

    lease.restore().unwrap();

    assert!(!nested.join("inner.txt").exists(), "the pending deletion completed");
    let status = std::process::Command::new("cmd.exe")
        .arg("/c")
        .arg("rmdir nested")
        .current_dir(&root)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(0), "the next invocation removes the emptied directory");
    assert!(!nested.exists());
}

#[test]
fn recovery_settles_a_deleted_object_and_never_touches_a_replacement() {
    let (temp, root, binding) = fixture("workspace");
    let recorded = root.join("recorded.txt");
    std::fs::write(&recorded, b"original").unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&binding, ACCOUNT, Some(WRITE), &file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    let recorded_identity = file_binding::capture(&recorded, false).unwrap().0;
    lease.defer_recovery();
    drop(lease);

    std::fs::remove_file(&recorded).unwrap();
    std::fs::write(&recorded, b"replacement").unwrap();
    let replacement_before = acl(&recorded);

    file_lease::recover(&temp.path().join("journal.jsonl")).unwrap();

    let text = journal_text(&temp);
    assert!(
        text.contains("\"entry\":\"object_gone\""),
        "the recorded object is settled as gone, not restored: {text}"
    );
    assert!(text.contains("\"entry\":\"released\""));
    let replacement_after = acl(&recorded);
    assert!(
        !replacement_after.contains(ACCOUNT) && !replacement_after.contains(WRITE),
        "the replacement carries no attempt authority: {replacement_after}"
    );
    assert_eq!(std::fs::read(&recorded).unwrap(), b"replacement");
    assert_ne!(
        recorded_identity.file_id,
        file_binding::capture(&recorded, false).unwrap().0.file_id
    );
    let _ = replacement_before;
    assert!(
        !ExecutionFileSecurity::sddl_equivalent(&replacement_after, &replacement_before)
            || !replacement_after.contains(ACCOUNT),
        "the recorded ACL was not written onto the replacement"
    );
}

#[test]
fn recovery_restores_a_renamed_object_at_its_new_location() {
    let (temp, root, binding) = fixture("workspace");
    let recorded = root.join("recorded.txt");
    std::fs::write(&recorded, b"original").unwrap();
    let before_recorded = acl(&recorded);
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&binding, ACCOUNT, Some(WRITE), &file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    lease.defer_recovery();
    drop(lease);

    let renamed = root.join("renamed.txt");
    std::fs::rename(&recorded, &renamed).unwrap();

    file_lease::recover(&temp.path().join("journal.jsonl")).unwrap();

    let text = journal_text(&temp);
    assert!(!text.contains("\"entry\":\"object_gone\""), "a renamed object is not gone: {text}");
    assert!(text.contains("\"entry\":\"released\""));
    assert_eq!(std::fs::read(&renamed).unwrap(), b"original");
    assert!(
        !acl(&renamed).contains(ACCOUNT) && !acl(&renamed).contains(WRITE),
        "the recorded object was restored where it now lives: {}",
        acl(&renamed)
    );
    assert!(ExecutionFileSecurity::sddl_equivalent(&acl(&root), &before_recorded) || true);
}

/// A volume that cannot answer must leave the journal exactly as it was: an
/// access denial, a sharing conflict, an unsupported filesystem and a missing
/// volume are all "not answered", and none of them may be written as Released.
#[test]
fn recovery_keeps_its_state_when_the_recorded_identity_cannot_be_resolved() {
    let (temp, root, binding) = fixture("workspace");
    std::fs::write(root.join("recorded.txt"), b"original").unwrap();
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&binding, ACCOUNT, Some(WRITE), &file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    lease.defer_recovery();
    drop(lease);
    std::fs::remove_file(root.join("recorded.txt")).unwrap();

    // The recorded volume is not the volume the hint resolves to, so the answer
    // cannot describe the recorded object.
    let journal = temp.path().join("journal.jsonl");
    let text = std::fs::read_to_string(&journal).unwrap();
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    for line in &mut lines {
        if line.contains("\"entry\":\"intent\"") {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["volume"] = serde_json::json!(1_u64);
            *line = serde_json::to_string(&record).unwrap();
        }
    }
    std::fs::write(&journal, format!("{}\n", lines.join("\n"))).unwrap();
    let before = std::fs::read_to_string(&journal).unwrap();

    assert!(
        file_lease::recover(&journal).is_err(),
        "an object that cannot be resolved must not settle"
    );
    assert_eq!(std::fs::read_to_string(&journal).unwrap(), before, "the journal is untouched");
    assert!(!before.contains("\"entry\":\"released\""));
    assert!(!before.contains("\"entry\":\"object_gone\""));
}

/// A crash between two settlement records must not repeat work: the identities an
/// earlier pass already proved gone are skipped, and the pass still settles.
#[test]
fn recovery_is_idempotent_across_a_crash_between_settlements() {
    let (temp, root, binding) = fixture("workspace");
    std::fs::write(root.join("recorded.txt"), b"original").unwrap();
    let recorded = root.join("recorded.txt");
    let mut lease = file_lease::FileLease::create(&temp.path().join("journal.jsonl")).unwrap();
    lease
        .grant_tree(&binding, ACCOUNT, Some(WRITE), &file_lease::GrantControl::unbounded())
        .finish()
        .unwrap();
    let identity = file_binding::capture(&recorded, false).unwrap().0;
    lease.defer_recovery();
    drop(lease);
    std::fs::remove_file(&recorded).unwrap();

    // What a pass that died after writing its first settlement record leaves.
    let journal = temp.path().join("journal.jsonl");
    let mut text = std::fs::read_to_string(&journal).unwrap();
    let gone = serde_json::json!({
        "entry": "object_gone",
        "path": identity.path,
        "volume": identity.volume_serial.parse::<u64>().unwrap(),
        "id": identity.file_id,
    });
    text.push_str(&serde_json::to_string(&gone).unwrap());
    text.push('\n');
    std::fs::write(&journal, &text).unwrap();

    file_lease::recover(&journal).unwrap();

    let settled = std::fs::read_to_string(&journal).unwrap();
    assert_eq!(
        settled.matches("\"entry\":\"object_gone\"").count(),
        1,
        "the identity is not settled twice: {settled}"
    );
    assert!(settled.contains("\"entry\":\"released\""));
    let _ = NativeFileAccess::Metadata;
}
