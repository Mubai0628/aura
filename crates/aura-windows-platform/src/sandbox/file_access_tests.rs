#![allow(clippy::unwrap_used)]
use super::file_access::ExecutionFileSecurity;
use std::path::Path;
use std::ptr;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Security::Authorization::{
    GRANT_ACCESS, GetNamedSecurityInfoW, SE_FILE_OBJECT, SetEntriesInAclW, SetNamedSecurityInfoW,
};
use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, SUB_CONTAINERS_AND_OBJECTS_INHERIT};

const TEST_SID: &str = "S-1-15-3-171717-282828-393939";
const FULL_CONTROL: u32 = 0x001f_01ff;

#[test]
fn fixed_online_read_plan_preserves_private_children_and_rejects_null_dacl() {
    let temp = tempfile::tempdir().unwrap();
    let scope = temp.path().join("package");
    std::fs::create_dir(&scope).unwrap();
    let private = scope.join("private");
    std::fs::create_dir(&private).unwrap();
    let private = ExecutionFileSecurity::open(&private, true).unwrap();
    let before_private = private.capture().unwrap();
    let object = ExecutionFileSecurity::open(&scope, true).unwrap();
    let before = object.capture().unwrap();
    let after = ExecutionFileSecurity::read_only_plan(&before, TEST_SID).unwrap();
    assert_eq!(after, object.plan(&[(TEST_SID, 0x1200a9)]).unwrap());
    object.apply_exact(&before, &after).unwrap();
    assert_eq!(private.capture().unwrap(), before_private);
    assert!(!after.contains(&format!("OI;GR;;;{TEST_SID}")));
    let created = scope.join("new.txt");
    std::fs::write(&created, "must not inherit package read grant").unwrap();
    assert!(
        !ExecutionFileSecurity::open(&created, false)
            .unwrap()
            .capture()
            .unwrap()
            .contains(TEST_SID)
    );
    object.apply_exact(&after, &before).unwrap();
    assert!(ExecutionFileSecurity::sddl_equivalent(&object.capture().unwrap(), &before));
    assert!(ExecutionFileSecurity::read_only_plan("D:NO_ACCESS_CONTROL", TEST_SID).is_err());
}

#[test]
fn exact_acl_lease_restores_renamed_object_without_touching_children() {
    let temp = tempfile::tempdir().unwrap();
    let directory = temp.path().join("scope");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("before.txt");
    std::fs::write(&path, "same object").unwrap();
    let root = ExecutionFileSecurity::open(&directory, true).unwrap();
    let file = ExecutionFileSecurity::open(&path, false).unwrap();
    let before_root = root.capture().unwrap();
    let before_file = file.capture().unwrap();
    let next = root.plan_with_inheritance(&[(TEST_SID, 0x0012_0089)], true).unwrap();
    root.apply_exact(&before_root, &next).unwrap_or_else(|e| {
        panic!("{e}: before={before_root}; planned={next}; actual={:?}", root.capture())
    });
    assert_eq!(file.capture().unwrap(), before_file, "root grant must not propagate");
    root.apply_exact(&next, &before_root).unwrap();
    let next_file = file.plan(&[(TEST_SID, 0x0013_01bf)]).unwrap();
    file.apply_exact(&before_file, &next_file).unwrap();
    let after = directory.join("after.txt");
    std::fs::rename(&path, &after).unwrap();
    file.apply_exact(&next_file, &before_file).unwrap();
    assert!(ExecutionFileSecurity::sddl_equivalent(
        &ExecutionFileSecurity::open(&after, false).unwrap().capture().unwrap(),
        &before_file
    ));
    assert!(ExecutionFileSecurity::sddl_equivalent(&root.capture().unwrap(), &before_root));
}

#[test]
fn lease_rejects_hardlink_and_preserves_foreign_acl_change() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("single.txt");
    std::fs::write(&path, "data").unwrap();
    let file = ExecutionFileSecurity::open(&path, false).unwrap();
    let before = file.capture().unwrap();
    let ours = file.plan(&[(TEST_SID, 0x0012_0089)]).unwrap();
    file.apply_exact(&before, &ours).unwrap_or_else(|e| {
        panic!("{e}: before={before}; planned={ours}; actual={:?}", file.capture())
    });
    let foreign = file.plan(&[("S-1-15-3-111111-222222-333333", 0x0012_0089)]).unwrap();
    file.apply_exact(&ours, &foreign).unwrap();
    assert!(file.apply_exact(&ours, &before).is_err());
    assert_eq!(file.capture().unwrap(), foreign);
    file.apply_exact(&foreign, &before).unwrap();
    std::fs::hard_link(&path, temp.path().join("alias.txt")).unwrap();
    assert!(ExecutionFileSecurity::open(&path, false).is_err());
}

#[test]
fn publish_parent_without_delete_share_blocks_maximum_allowed_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let parent = crate::open_verified_publish_parent(temp.path()).unwrap();
    let error = match crate::path::open_native_with_desired_access(
        temp.path(),
        0x0200_0000,
        false,
        windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
            | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
    ) {
        Ok(_) => panic!("MAXIMUM_ALLOWED reopen must not share with publish parent"),
        Err(error) => error,
    };
    assert_eq!(error.raw_os_error(), Some(32), "{error}");
    ExecutionFileSecurity::open(temp.path(), true).unwrap();
    drop(parent);
}

#[test]
fn acl_lease_allows_read_share_without_write_share() {
    use std::os::windows::fs::OpenOptionsExt;
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("input.txt");
    std::fs::write(&path, "synthetic authorized input").unwrap();
    let lease = ExecutionFileSecurity::open(&path, false).unwrap();
    let mut file = std::fs::OpenOptions::new();
    file.read(true).share_mode(windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ);
    let mut opened = file.open(&path).expect("FileShare.Read must succeed during ACL lease");
    let mut text = String::new();
    std::io::Read::read_to_string(&mut opened, &mut text).unwrap();
    assert_eq!(text, "synthetic authorized input");
    drop(lease);
}

#[test]
fn allow_ace_order_does_not_change_dacl_meaning() {
    let first = "D:PAI(A;;0x1200a9;;;S-1-5-21-1-2-3-1015)(A;OICI;FA;;;SY)";
    let second = "D:PAI(A;OICI;FA;;;SY)(A;;0x1200a9;;;S-1-5-21-1-2-3-1015)";
    assert!(ExecutionFileSecurity::sddl_equivalent(first, second));
    assert!(!ExecutionFileSecurity::sddl_equivalent(
        "D:(D;;GA;;;WD)(A;;FA;;;SY)",
        "D:(A;;FA;;;SY)(D;;GA;;;WD)"
    ));
    assert!(ExecutionFileSecurity::sddl_equivalent("D:P(A;OICI;FA;;;SY)", "D:PAI(A;OICI;FA;;;SY)"));
}

/// Merge one allow ACE into the object's live DACL through the named API.
/// Fixture setup only: the public planning API never grants FullControl, while
/// the retired-output history this reproduces left exactly such grants behind.
fn named_grant(path: &Path, sid: &str, mask: u32, inheritance: u32) {
    let (entry, _sid) =
        super::explicit_access_for_sid(sid, mask, GRANT_ACCESS, inheritance).unwrap();
    let path_wide = super::wide_null(&path.to_string_lossy());
    let mut current = ptr::null_mut();
    let mut descriptor: windows_sys::Win32::Security::PSECURITY_DESCRIPTOR = ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            path_wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut current,
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "GetNamedSecurityInfoW({})", path.display());
    let _descriptor = super::LocalAllocation(descriptor.cast());
    let mut merged = ptr::null_mut();
    let status = unsafe { SetEntriesInAclW(1, &raw const entry, current, &mut merged) };
    assert_eq!(status, ERROR_SUCCESS, "SetEntriesInAclW({})", path.display());
    let _merged = super::LocalAllocation(merged.cast());
    let status = unsafe {
        SetNamedSecurityInfoW(
            path_wide.as_ptr().cast_mut(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            merged,
            ptr::null_mut(),
        )
    };
    assert_eq!(status, ERROR_SUCCESS, "SetNamedSecurityInfoW({})", path.display());
}

#[test]
fn explicit_plus_inherited_grant_plans_the_acl_windows_retains() {
    // The v4 counterexample: a file that carries its own explicit FullControl
    // grant for the controller *and* inherits the same grant from its parent.
    // Planning converted the inherited ACE to explicit, producing two identical
    // allow ACEs; Windows kept only one on write, so the postcondition and the
    // restore both failed after the object had already been changed.
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("scope");
    std::fs::create_dir(&parent).unwrap();
    let (controller, _) = crate::current_user_token_identity().unwrap();
    named_grant(&parent, &controller, FULL_CONTROL, SUB_CONTAINERS_AND_OBJECTS_INHERIT);
    let path = parent.join("payload.txt");
    std::fs::write(&path, "content must survive the lease").unwrap();
    named_grant(&path, &controller, FULL_CONTROL, 0);

    let object = ExecutionFileSecurity::open(&path, false).unwrap();
    let parent_acl = ExecutionFileSecurity::open(&parent, true).unwrap();
    let parent_before = parent_acl.capture().unwrap();
    let before = object.capture().unwrap();
    assert_eq!(before.matches(&format!("(A;ID;FA;;;{controller})")).count(), 1, "{before}");
    assert_eq!(before.matches(&format!("(A;;FA;;;{controller})")).count(), 1, "{before}");

    let planned = object.plan_for_attempt(TEST_SID, Some("S-1-15-3-444444-555555-666666")).unwrap();
    assert_eq!(
        planned.matches(&format!("(A;;FA;;;{controller})")).count(),
        1,
        "the plan must be the ACL Windows retains, duplicates already collapsed: {planned}"
    );
    object
        .apply_exact(&before, &planned)
        .unwrap_or_else(|e| panic!("{e}: before={before}; planned={planned}"));
    let observed = object.capture().unwrap();
    assert!(ExecutionFileSecurity::sddl_equivalent(&observed, &planned));

    object.apply_exact(&observed, &before).unwrap();
    assert!(ExecutionFileSecurity::sddl_equivalent(&object.capture().unwrap(), &before));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "content must survive the lease");
    assert_eq!(parent_acl.capture().unwrap(), parent_before);
}

#[test]
fn modern_protected_output_retires_to_ordinary_creation_permissions() {
    assert_created_permissions(true, true);
}

#[test]
fn modern_inherited_output_retires_to_ordinary_creation_permissions() {
    assert_created_permissions(true, false);
}

#[test]
fn legacy_protected_output_retires_to_ordinary_creation_permissions() {
    assert_created_permissions(false, true);
}

#[test]
fn legacy_inherited_output_retires_to_ordinary_creation_permissions() {
    assert_created_permissions(false, false);
}

fn assert_created_permissions(modern: bool, protected: bool) {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("base");
    std::fs::create_dir(&base).unwrap();
    let (controller, _) = crate::current_user_token_identity().unwrap();
    let mode = if modern { "PAI" } else { "P" };
    let set_limited = |path: &Path| {
        let object = ExecutionFileSecurity::open(path, false).unwrap();
        object
            .apply_exact(
                &object.capture().unwrap(),
                &format!("D:{mode}(A;OICI;0x1301bf;;;{controller})"),
            )
            .unwrap();
    };
    set_limited(&base);
    let scope = base.join("scope");
    std::fs::create_dir(&scope).unwrap();
    if protected {
        set_limited(&scope);
    }
    let clean_file = scope.join("clean.txt");
    let clean_directory = scope.join("clean-directory");
    std::fs::write(&clean_file, "ordinary content").unwrap();
    std::fs::create_dir(&clean_directory).unwrap();
    let existing = clean_directory.join("existing.txt");
    std::fs::write(&existing, "original content").unwrap();
    let capture =
        |path: &Path| ExecutionFileSecurity::open(path, false).unwrap().capture().unwrap();
    let originals: Vec<_> = [&clean_file, &clean_directory, &existing]
        .into_iter()
        .map(|path| (path, capture(path)))
        .collect();
    let object = ExecutionFileSecurity::open(&scope, true).unwrap();
    let before = object.capture().unwrap();
    let write = "S-1-15-3-444444-555555-666666";
    let plan = object.plan_for_attempt(TEST_SID, Some(write)).unwrap();
    object.apply_exact(&before, &plan).unwrap();
    for (path, acl) in &originals {
        assert_eq!(capture(path), *acl, "grant must not rewrite an original child");
    }
    let directory = scope.join("created-directory");
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("created.txt");
    std::fs::write(&file, "command content").unwrap();
    assert!(capture(&file).contains(write), "temporary access must really be inherited");
    object.apply_exact(&object.capture().unwrap(), &before).unwrap();
    assert_eq!(object.capture().unwrap(), before, "restore original inheritance mode");
    for path in [&directory, &file] {
        ExecutionFileSecurity::open(path, false)
            .unwrap()
            .retire_created_file(TEST_SID, write)
            .unwrap();
        let acl = capture(path);
        assert!(!acl.contains(TEST_SID) && !acl.contains(write), "{acl}");
        let ordinary = if path == &directory { &originals[1].1 } else { &originals[0].1 };
        assert!(ExecutionFileSecurity::sddl_equivalent(&acl, ordinary), "{acl} != {ordinary}");
    }
    for (path, acl) in &originals {
        assert_eq!(capture(path), *acl, "retirement must not rewrite an original child");
    }
    assert_eq!(std::fs::read_to_string(file).unwrap(), "command content");
    assert_eq!(std::fs::read_to_string(existing).unwrap(), "original content");
}

#[test]
fn retirement_preserves_a_legitimate_explicit_controller_grant() {
    let temp = tempfile::tempdir().unwrap();
    let (controller, _) = crate::current_user_token_identity().unwrap();
    let path = temp.path().join("protected-output.txt");
    std::fs::write(&path, "legitimate explicit authority").unwrap();
    let object = ExecutionFileSecurity::open(&path, false).unwrap();
    let write = "S-1-15-3-444444-555555-666666";
    let unrelated = "S-1-15-3-777777-888888-999999";
    let original = object.capture().unwrap();
    let intentional = format!(
        "D:P(A;;FA;;;{controller})(A;;0x1301bf;;;{TEST_SID})\
         (A;;0x1301bf;;;{write})(A;;FR;;;{unrelated})"
    );
    object.apply_exact(&original, &intentional).unwrap_or_else(|error| {
        panic!("{error}; planned={intentional}; actual={:?}", object.capture())
    });
    object.retire_created_file(TEST_SID, write).unwrap();
    let after = object.capture().unwrap();
    assert!(after.contains(&format!("(A;;FA;;;{controller})")), "{after}");
    assert!(after.contains(&format!("(A;;FR;;;{unrelated})")), "{after}");
    assert!(!after.contains(TEST_SID) && !after.contains(write), "{after}");
    assert!(after.starts_with("D:P"), "{after}");
    assert_eq!(std::fs::read_to_string(path).unwrap(), "legitimate explicit authority");
}

#[test]
fn retiring_created_directory_preserves_a_relocated_original_child() {
    let temp = tempfile::tempdir().unwrap();
    let scope = temp.path().join("scope");
    let private = scope.join("private");
    std::fs::create_dir_all(&private).unwrap();
    let (controller, _) = crate::current_user_token_identity().unwrap();
    let unrelated = "S-1-15-3-777777-888888-999999";
    let private_acl = ExecutionFileSecurity::open(&private, false).unwrap();
    private_acl
        .apply_exact(
            &private_acl.capture().unwrap(),
            &format!("D:PAI(A;OICI;0x1301bf;;;{controller})(A;OICI;FR;;;{unrelated})"),
        )
        .unwrap();
    let original = private.join("original.txt");
    std::fs::write(&original, "existing content").unwrap();
    let capture =
        |path: &Path| ExecutionFileSecurity::open(path, false).unwrap().capture().unwrap();
    let original_acl = capture(&original);
    let root = ExecutionFileSecurity::open(&scope, true).unwrap();
    let before = root.capture().unwrap();
    let write = "S-1-15-3-444444-555555-666666";
    let plan = root.plan_for_attempt(TEST_SID, Some(write)).unwrap();
    root.apply_exact(&before, &plan).unwrap();
    let directory = scope.join("new-directory");
    std::fs::create_dir(&directory).unwrap();
    let moved = directory.join("original.txt");
    std::fs::rename(&original, &moved).unwrap();
    assert_eq!(capture(&moved), original_acl, "moving preserves the original ACL");
    root.apply_exact(&root.capture().unwrap(), &before).unwrap();
    ExecutionFileSecurity::open(&directory, false)
        .unwrap()
        .retire_created_file(TEST_SID, write)
        .unwrap();
    assert_eq!(
        capture(&moved),
        original_acl,
        "retirement must not propagate onto an original object"
    );
    assert_eq!(std::fs::read_to_string(moved).unwrap(), "existing content");
}
