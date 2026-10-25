//! What a real AppContainer identity can do to a temporary object.
//!
//! The DACL tests next door compare descriptors. This module compares
//! *behaviour*: the ACE is granted through the same functions the host
//! preparation and the sandbox use, and the access is attempted by a real
//! AppContainer child whose token is checked — not by the administrator who
//! wrote the ACE, and not by reading the SDDL back.
//!
//! Every object here is temporary: the system drive, the NUL device and any
//! other existing object are never touched.

use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
    time::Duration,
};

use windows_sys::Win32::Storage::FileSystem::{
    FILE_EXECUTE, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_READ_DATA,
};

use super::{
    ALL_APPLICATION_PACKAGES_SID, GRANT_ACCESS, SYSTEM_DRIVE_COMPATIBILITY_MASK,
    dacl_write_tests::{
        dacl_control_flags, full_descriptor, identities, mark_protected, raw_control_bits,
        update_file_dacl_with_named_api,
    },
    update_file_dacl,
};
use crate::sandbox::{
    AppContainerProfile, NativeAclAccess, NativeEnvironmentEntry, NativeRestrictedMode,
    capture_file_dacl, delete_appcontainer_profile, grant_file_dacl, revoke_file_dacl,
    spawn_restricted_process,
};

const PAYLOAD_MARKER: &str = "AURA-ACL-MATRIX-PAYLOAD";
const CHILD_IMAGE: &str = r"C:\Windows\System32\cmd.exe";
/// What a sandbox attempt gets on its own directory: read and write, no
/// inheritance (the attempt directory is the whole subtree).
const READ_WRITE: u32 = FILE_GENERIC_READ | FILE_GENERIC_WRITE;
static PROFILE_SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// An AppContainer profile that removes itself when the test ends.
struct Profile {
    profile: AppContainerProfile,
    name: String,
}

impl Profile {
    fn fresh(label: &str) -> Self {
        let sequence = PROFILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let name = format!("Aura.AclMatrix.{label}.{}.{sequence}", std::process::id());
        let profile = AppContainerProfile::ensure(&name).expect("AppContainer profile");
        assert!(profile.was_created(), "this test needs a fresh profile: {name}");
        Self { profile, name }
    }

    fn sid(&self) -> String {
        self.profile.sid_string().expect("profile SID")
    }
}

impl Drop for Profile {
    fn drop(&mut self) {
        let _ = delete_appcontainer_profile(&self.name);
    }
}

/// A real AppContainer child that ran `cmd /d /c <command>` in `working_directory`.
struct ChildOutcome {
    exit_code: Option<u32>,
    stdout: String,
    stderr: String,
    token_is_appcontainer: bool,
}

impl ChildOutcome {
    fn saw_payload(&self) -> bool {
        self.stdout.contains(PAYLOAD_MARKER)
    }

    fn summary(&self) -> String {
        format!(
            "exit={:?} appcontainer={} stdout={:?} stderr={:?}",
            self.exit_code,
            self.token_is_appcontainer,
            self.stdout.trim(),
            self.stderr.trim()
        )
    }
}

fn utf16(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}

/// Launches the child with the same image, arguments and working directory every
/// time; only the object's DACL differs between the calls.
fn run_child(profile: &Profile, working_directory: &Path, command: &str) -> ChildOutcome {
    let environment = [NativeEnvironmentEntry {
        name: "LOCALAPPDATA".to_owned(),
        value_utf16: working_directory.to_string_lossy().encode_utf16().collect(),
    }];
    let process = spawn_restricted_process(
        NativeRestrictedMode::AppContainer,
        Some(&profile.profile),
        Path::new(CHILD_IMAGE),
        &[utf16("/d"), utf16("/c"), utf16(command)],
        working_directory,
        &environment,
    )
    .expect("launch the AppContainer child");
    let token_is_appcontainer = process.token_is_appcontainer().expect("query the child token");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = None;
    for _ in 0..500 {
        stdout.extend(process.drain_stdout(64 * 1024).expect("drain stdout"));
        stderr.extend(process.drain_stderr(64 * 1024).expect("drain stderr"));
        if let Some(code) = process.wait_timeout(Duration::from_millis(20)).expect("wait") {
            exit_code = Some(code);
            break;
        }
    }
    stdout.extend(process.drain_stdout(64 * 1024).expect("final stdout"));
    stderr.extend(process.drain_stderr(64 * 1024).expect("final stderr"));
    ChildOutcome {
        exit_code,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
        token_is_appcontainer,
    }
}

/// A temporary directory holding `payload.txt`, plus the nested directory and
/// file the inheritance cases use.
struct TemporaryTree {
    root: tempfile::TempDir,
}

impl TemporaryTree {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("temporary directory");
        std::fs::write(root.path().join("payload.txt"), PAYLOAD_MARKER).expect("write payload");
        std::fs::create_dir(root.path().join("nested")).expect("nested directory");
        std::fs::write(root.path().join("nested").join("payload.txt"), PAYLOAD_MARKER)
            .expect("write the nested payload");
        Self { root }
    }

    fn path(&self) -> &Path {
        self.root.path()
    }

    fn payload(&self) -> PathBuf {
        self.root.path().join("payload.txt")
    }

    fn nested_payload(&self) -> PathBuf {
        self.root.path().join("nested").join("payload.txt")
    }
}

fn assert_child_identity(outcome: &ChildOutcome, what: &str) {
    assert!(
        outcome.token_is_appcontainer,
        "the {what} measurement must come from an AppContainer token, got {}",
        outcome.summary()
    );
}

fn assert_denied(outcome: &ChildOutcome, what: &str) {
    assert_child_identity(outcome, what);
    assert!(
        !outcome.saw_payload(),
        "the container read the payload when it should not: {}",
        outcome.summary()
    );
    assert_ne!(outcome.exit_code, Some(0), "the child reported success: {}", outcome.summary());
}

fn assert_allowed(outcome: &ChildOutcome, what: &str) {
    assert_child_identity(outcome, what);
    assert_eq!(
        outcome.exit_code,
        Some(0),
        "the container could not read what it was granted: {}",
        outcome.summary()
    );
    assert!(outcome.saw_payload(), "the payload was not returned: {}", outcome.summary());
}

#[test]
fn a_real_appcontainer_identity_reads_only_while_the_grant_is_present() {
    // The negative case comes first, so the same object is measured before and
    // after the write: what changes is the DACL, not the object, the image, the
    // arguments, the working directory or the profile.
    let tree = TemporaryTree::new();
    let profile = Profile::fresh("Read");
    let sid = profile.sid();

    assert_denied(&run_child(&profile, tree.path(), "type payload.txt"), "read before the grant");
    let listing = run_child(&profile, tree.path(), "dir /b");
    assert_child_identity(&listing, "listing before the grant");
    assert_ne!(listing.exit_code, Some(0), "listing was allowed: {}", listing.summary());

    grant_file_dacl(tree.path(), &sid, NativeAclAccess::ReadWrite, true)
        .expect("grant the container SID");
    assert_allowed(&run_child(&profile, tree.path(), "type payload.txt"), "read after the grant");

    // Idempotence: a second grant leaves the object and the behaviour as they are.
    let after_first = full_descriptor(tree.path());
    grant_file_dacl(tree.path(), &sid, NativeAclAccess::ReadWrite, true)
        .expect("grant the container SID again");
    assert_eq!(full_descriptor(tree.path()), after_first, "the second grant must be a no-op");
    assert_allowed(&run_child(&profile, tree.path(), "type payload.txt"), "read after regranting");

    // The restore path: the ACE leaves with the sandbox that owned it, and the
    // access goes back to what it was before the grant.
    revoke_file_dacl(tree.path(), &sid, true).expect("revoke the container SID");
    let revoked = capture_file_dacl(tree.path()).expect("capture the DACL");
    assert!(!revoked.contains(&sid), "the revoked ACE is still present: {revoked}");
    assert_denied(&run_child(&profile, tree.path(), "type payload.txt"), "read after the revoke");
}

#[test]
fn an_inheritable_grant_reaches_children_created_before_and_after_it() {
    // `C:\` carries a non-inheritable ACE, while an attempt directory is granted
    // recursively. Both directions are measured here: descendants that already
    // existed when the ACE was written, and descendants created afterwards.
    let tree = TemporaryTree::new();
    let profile = Profile::fresh("Inherit");
    let sid = profile.sid();
    assert_denied(
        &run_child(&profile, tree.path(), "type nested\\payload.txt"),
        "nested read before the grant",
    );

    grant_file_dacl(tree.path(), &sid, NativeAclAccess::ReadWrite, true)
        .expect("grant the container SID");
    assert_allowed(
        &run_child(&profile, tree.path(), "type nested\\payload.txt"),
        "nested read after the grant",
    );

    std::fs::create_dir(tree.path().join("fresh")).expect("fresh directory");
    std::fs::write(tree.path().join("fresh").join("payload.txt"), PAYLOAD_MARKER)
        .expect("write the fresh payload");
    assert_allowed(
        &run_child(&profile, tree.path(), "type fresh\\payload.txt"),
        "read below a directory created after the grant",
    );

    let nested = full_descriptor(&tree.path().join("nested"));
    let fresh = full_descriptor(&tree.path().join("fresh"));
    assert!(
        nested.contains(&sid) && fresh.contains(&sid),
        "the inheritable ACE must appear in both children: {nested} / {fresh}"
    );
}

#[test]
fn the_drive_root_compatibility_mask_grants_no_file_data_access() {
    // The mask granted on the system drive is a compatibility grant, not a data
    // grant: it carries neither `FILE_READ_DATA` nor `FILE_EXECUTE`, so it cannot
    // be what lets an attempt read its own files.
    assert_eq!(
        SYSTEM_DRIVE_COMPATIBILITY_MASK & (FILE_READ_DATA | FILE_EXECUTE),
        0,
        "the compatibility mask must stay a metadata-only grant"
    );

    let tree = TemporaryTree::new();
    let profile = Profile::fresh("Mask");
    let sid = profile.sid();
    for path in [tree.path(), tree.payload().as_path()] {
        update_file_dacl(path, &sid, SYSTEM_DRIVE_COMPATIBILITY_MASK, GRANT_ACCESS, 0, &[])
            .expect("write the compatibility ACE");
    }
    assert!(
        full_descriptor(tree.path()).contains(&sid),
        "the ACE has to be on the object for this measurement to mean anything"
    );
    let outcome = run_child(&profile, tree.path(), "type payload.txt");
    assert_denied(&outcome, "read with only the compatibility mask");
    println!("compatibility mask only: {}", outcome.summary());
}

#[test]
fn the_auto_inherited_marker_changes_no_access_outcome() {
    // Two temporary objects, one granted through the targeted write the host
    // preparation uses and one through the inheritance-passing API the sandbox
    // uses. The descriptors differ in the Windows-owned auto-inherited marker;
    // the access a real container gets must not.
    let targeted = TemporaryTree::new();
    let named = TemporaryTree::new();
    let profile = Profile::fresh("Marker");
    let sid = profile.sid();

    update_file_dacl(targeted.path(), &sid, READ_WRITE, GRANT_ACCESS, 0, &[])
        .expect("targeted grant on the directory");
    update_file_dacl(&targeted.payload(), &sid, READ_WRITE, GRANT_ACCESS, 0, &[])
        .expect("targeted grant on the payload");
    update_file_dacl_with_named_api(named.path(), &sid, READ_WRITE);
    update_file_dacl_with_named_api(&named.payload(), &sid, READ_WRITE);

    let targeted_flags = dacl_control_flags(&full_descriptor(targeted.path()));
    let named_flags = dacl_control_flags(&full_descriptor(named.path()));
    println!("control flags: targeted={targeted_flags:?} named={named_flags:?}");
    assert!(!targeted_flags.contains("AI"), "the targeted write drops the marker");
    assert!(named_flags.contains("AI"), "the inheritance pass keeps the marker");

    assert_allowed(&run_child(&profile, targeted.path(), "type payload.txt"), "targeted grant");
    assert_allowed(&run_child(&profile, named.path(), "type payload.txt"), "named grant");

    // Both writes produced the same ACE list, so the only difference under test
    // is the marker itself.
    assert_eq!(
        identities(&full_descriptor(targeted.path())),
        identities(&full_descriptor(named.path())),
        "the two writes must produce the same ACE list"
    );
}

#[test]
fn an_explicit_grant_still_works_on_a_protected_object() {
    // `SE_DACL_PROTECTED` stops a parent's inheritable ACEs from reaching the
    // object; it does not stop an explicit ACE from working. The two bits are
    // different things, and only one of them is about access.
    let tree = TemporaryTree::new();
    let profile = Profile::fresh("Protected");
    let sid = profile.sid();

    mark_protected(tree.path());
    assert_ne!(
        raw_control_bits(tree.path()) & windows_sys::Win32::Security::SE_DACL_PROTECTED,
        0,
        "the object should be protected before the grant"
    );

    grant_file_dacl(tree.path(), &sid, NativeAclAccess::ReadWrite, true)
        .expect("grant the container SID");
    assert_allowed(&run_child(&profile, tree.path(), "type payload.txt"), "read while protected");
    assert_ne!(
        raw_control_bits(tree.path()) & windows_sys::Win32::Security::SE_DACL_PROTECTED,
        0,
        "the grant must not unprotect the object"
    );
}

/// Measurement, not a gate: the facts the assertions above are written from.
/// Run with
/// `cargo test -p aura-windows-platform --lib measure_appcontainer_access -- --ignored --nocapture`.
#[test]
#[ignore = "measurement: run explicitly"]
fn measure_appcontainer_access_to_a_temporary_object() {
    let tree = TemporaryTree::new();
    let profile = Profile::fresh("Measure");
    let sid = profile.sid();
    println!("sid={sid} root={}", tree.path().display());

    for (label, command) in [
        ("relative read, no grant", "type payload.txt"),
        ("relative read through a child directory, no grant", "type nested\\payload.txt"),
        ("listing, no grant", "dir /b"),
        ("identity, no grant", "C:\\Windows\\System32\\whoami.exe /groups"),
    ] {
        println!("{label}: {}", run_child(&profile, tree.path(), command).summary());
    }

    grant_file_dacl(tree.path(), &sid, NativeAclAccess::ReadWrite, true)
        .expect("grant the container SID");
    std::fs::create_dir(tree.path().join("fresh")).expect("fresh directory");
    std::fs::write(tree.path().join("fresh").join("payload.txt"), PAYLOAD_MARKER)
        .expect("write the fresh payload");
    for (label, command) in [
        ("relative read, granted", "type payload.txt"),
        (
            "relative read through a pre-existing child directory, granted",
            "type nested\\payload.txt",
        ),
        ("relative read through a directory created after the grant", "type fresh\\payload.txt"),
        ("create a file, granted", "echo x> created.txt"),
        ("read back the created file, granted", "type created.txt"),
        ("identity, granted", "C:\\Windows\\System32\\whoami.exe /groups"),
    ] {
        println!("{label}: {}", run_child(&profile, tree.path(), command).summary());
    }
    for (label, path) in [
        ("root", tree.path().to_path_buf()),
        ("nested", tree.path().join("nested")),
        ("nested/payload", tree.nested_payload()),
        ("fresh", tree.path().join("fresh")),
        ("fresh/payload", tree.path().join("fresh").join("payload.txt")),
    ] {
        println!("{label} dacl after grant: {}", capture_file_dacl(&path).expect("capture"));
    }

    revoke_file_dacl(tree.path(), &sid, true).expect("revoke the container SID");
    println!(
        "relative read, revoked: {}",
        run_child(&profile, tree.path(), "type payload.txt").summary()
    );

    // `ALL_APPLICATION_PACKAGES_SID` is the principal the host preparation writes
    // on the drive root; it is recorded here because the same SID is what the
    // sandbox grants per attempt.
    println!("compatibility principal under test: {ALL_APPLICATION_PACKAGES_SID}");
}
