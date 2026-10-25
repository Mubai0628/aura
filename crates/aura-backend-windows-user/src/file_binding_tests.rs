#![allow(clippy::unwrap_used)]
use super::file_binding;
use aura_domain_common::ContentHash;
use std::fs;

fn expect_err<T>(result: Result<T, anyhow::Error>) -> anyhow::Error {
    match result {
        Ok(_) => panic!("expected error"),
        Err(error) => error,
    }
}

#[test]
fn owned_hardlinked_file_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owned.bin");
    fs::write(&path, b"owned-bytes").unwrap();
    fs::hard_link(&path, temp.path().join("alias.bin")).unwrap();
    let error = expect_err(file_binding::capture(&path, true));
    assert!(error.to_string().contains("runner.aliased_file"), "{error}");
}

/// An MCP server's program may have other hard links, as Git's may. The
/// retained handle still refuses writes through every link, and a change made through another
/// link while nothing holds the file is caught when the binding is verified again.
#[test]
fn a_linked_image_is_held_against_writes_through_its_other_link() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("server.exe");
    let other = temp.path().join("other.exe");
    fs::write(&path, b"server-bytes").unwrap();
    fs::hard_link(&path, &other).unwrap();
    let (binding, held) = file_binding::capture_linked_image(&path).unwrap();
    assert!(binding.sha256.is_some());
    let refused = fs::OpenOptions::new().write(true).open(&other);
    assert_eq!(refused.unwrap_err().raw_os_error(), Some(32), "sharing violation");
    drop(held);
    drop(file_binding::verify_linked_image(&binding).unwrap());
    fs::write(&other, b"server-bytez").unwrap();
    let error = expect_err(file_binding::verify_linked_image(&binding));
    assert!(error.to_string().contains("runner.file_binding_changed"), "{error}");
    let error = expect_err(file_binding::verify(&binding));
    assert!(error.to_string().contains("runner.aliased_file"), "{error}");
}

#[test]
fn windows_cmd_image_can_be_bound_despite_winsxs_links() {
    let cmd = aura_windows_platform::windows_directory().unwrap().join("System32/cmd.exe");
    let (binding, _) = file_binding::capture(&cmd, true).unwrap();
    assert!(binding.sha256.is_some());
    assert!(
        binding
            .path
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case("cmd.exe"))
    );
    assert_ne!(
        binding.path.as_str(),
        cmd.to_string_lossy().as_ref(),
        "published CMD bindings keep the VOLUME_NAME_DOS prefix"
    );
    file_binding::verify(&binding).unwrap();
    file_binding::verify_system_cmd(&binding).unwrap();
}

#[test]
fn trusted_system_cmd_hash_change_is_serviced_not_ignored() {
    let cmd = aura_windows_platform::windows_directory().unwrap().join("System32/cmd.exe");
    let (live, _) = file_binding::capture(&cmd, true).unwrap();
    let mut recorded = live.clone();
    recorded.file_id = [0xff; 16];
    recorded.sha256 = Some(ContentHash::sha256(b"previous-system-cmd"));
    let error = expect_err(file_binding::evaluate_system_cmd(&recorded, |_, _| Ok(())));
    assert!(error.to_string().contains("runner.system_cmd_serviced"), "{error}");
    assert!(!error.to_string().contains("file_binding_changed"), "{error}");
    let exact = expect_err(file_binding::verify(&recorded));
    assert!(exact.to_string().contains("runner.file_binding_changed"), "{exact}");
}

#[test]
fn system_cmd_path_or_volume_change_is_not_servicing() {
    let cmd = aura_windows_platform::windows_directory().unwrap().join("System32/cmd.exe");
    let (live, _) = file_binding::capture(&cmd, true).unwrap();
    let mut volume = live.clone();
    volume.volume_serial = "0".into();
    volume.file_id = [0xee; 16];
    let error = expect_err(file_binding::evaluate_system_cmd(&volume, |_, _| Ok(())));
    assert!(error.to_string().contains("runner.file_binding_changed"), "{error}");
    let temp = tempfile::tempdir().unwrap();
    let copy = temp.path().join("cmd.exe");
    fs::copy(&cmd, &copy).unwrap();
    let (copied, _) = file_binding::capture(&copy, true).unwrap();
    let error = expect_err(file_binding::evaluate_system_cmd(&copied, |_, _| Ok(())));
    assert!(error.to_string().contains("runner.invalid_cmd_image"), "{error}");
}

#[test]
fn catalog_rejection_is_not_treated_as_servicing() {
    let cmd = aura_windows_platform::windows_directory().unwrap().join("System32/cmd.exe");
    let (live, _) = file_binding::capture(&cmd, true).unwrap();
    let error = expect_err(file_binding::evaluate_system_cmd(&live, |_, _| {
        Err(anyhow::anyhow!("runner.untrusted_cmd_image"))
    }));
    assert!(error.to_string().contains("runner.untrusted_cmd_image"), "{error}");
    assert!(!error.to_string().contains("system_cmd_serviced"), "{error}");
}
