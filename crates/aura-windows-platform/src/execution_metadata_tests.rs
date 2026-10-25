#![allow(clippy::unwrap_used)]
use super::*;
use crate::ExecutionFileSecurity;
const ACCOUNT: &str = "S-1-5-21-333333-444444-555555-666666";

fn synthetic_context() -> ExecutionMetadataAccess {
    let mut context = ExecutionMetadataAccess {
        context: ptr::null_mut(),
        manager: ptr::null_mut(),
        sid: ACCOUNT.into(),
    };
    let sid_text = ACCOUNT.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut sid = ptr::null_mut();
    unsafe {
        assert_ne!(ConvertStringSidToSidW(sid_text.as_ptr(), &mut sid), 0);
        assert_ne!(
            AuthzInitializeResourceManager(
                AUTHZ_RM_FLAG_NO_AUDIT,
                None,
                None,
                None,
                ptr::null(),
                &mut context.manager
            ),
            0
        );
        // Test-only context: do not query a machine/domain for this synthetic SID.
        assert_ne!(
            AuthzInitializeContextFromSid(
                AUTHZ_SKIP_TOKEN_GROUPS,
                sid,
                context.manager,
                ptr::null(),
                LUID::default(),
                ptr::null(),
                &mut context.context
            ),
            0
        );
        windows_sys::Win32::Foundation::LocalFree(sid);
    }
    context
}

#[test]
fn metadata_plan_is_noninheriting_and_does_not_override_an_explicit_deny() {
    let tmp = tempfile::tempdir().unwrap();
    let directory = tmp.path().join("private");
    std::fs::create_dir(&directory).unwrap();
    let private = directory.join("private.txt");
    std::fs::write(&private, "content").unwrap();
    let object = ExecutionFileSecurity::open(&directory, true).unwrap();
    let before = object.capture().unwrap();
    let child = ExecutionFileSecurity::open(&private, false).unwrap();
    let child_before = child.capture().unwrap();
    let context = synthetic_context();
    let (pin, allowed) = context.pin_directory(&directory).unwrap();
    assert!(!allowed);
    let after = object.plan(&[(ACCOUNT, 0x80)]).unwrap();
    assert!(context.allows_plan(&pin, &after).unwrap());
    // Windows may render mask 0x80 as the SDDL abbreviation LO.
    assert!(
        after.contains(&format!("(A;;LO;;;{ACCOUNT})"))
            || after.contains(&format!("(A;;0x80;;;{ACCOUNT})")),
        "{after}"
    );
    object.apply_exact(&before, &after).unwrap();
    assert!(context.pin_directory(&directory).unwrap().1);
    assert_eq!(child.capture().unwrap(), child_before);
    object.apply_exact(&after, &before).unwrap();
    let deny = format!("D:P(D;;0x80;;;{ACCOUNT})(A;;FA;;;SY)(A;;FA;;;BA)");
    assert!(!context.allows_plan(&pin, &deny).unwrap());
    let deny_and_allow = format!("{deny}(A;;0x80;;;{ACCOUNT})");
    assert!(!context.allows_plan(&pin, &deny_and_allow).unwrap());
}

#[test]
fn real_token_context_binds_process_and_does_not_mutate_readable_ancestors() {
    let current = crate::snapshot_process(std::process::id()).unwrap();
    let (sid, _) = crate::current_user_token_identity().unwrap();
    let access = ExecutionMetadataAccess::for_runner(&current, &sid).unwrap();
    let tmp = tempfile::tempdir().unwrap();
    assert!(access.pin_directory(tmp.path()).unwrap().1);
    assert!(ExecutionMetadataAccess::for_runner(&current, ACCOUNT).is_err());
    let mut stale = current;
    stale.creation_time_100ns += 1;
    assert!(ExecutionMetadataAccess::for_runner(&stale, &sid).is_err());
}
