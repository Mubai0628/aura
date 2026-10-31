//! What an isolated render does to the user's own Credential Manager entries.
use super::*;
use std::collections::BTreeMap;

use windows_sys::Win32::Security::Credentials::{CREDENTIALW, CredEnumerateW, CredFree};

/// The last-written time of each of the user's Microsoft account credentials, by target. No value
/// is read.
fn account_write_times() -> BTreeMap<String, u64> {
    let filter = "MicrosoftAccount:*".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut count = 0_u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: the filter is NUL-terminated and both outputs are writable.
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut list) } == 0 {
        return BTreeMap::new();
    }
    let mut times = BTreeMap::new();
    // SAFETY: CredEnumerateW returned `count` credential pointers that live until CredFree.
    for entry in unsafe { std::slice::from_raw_parts(list, count as usize) } {
        // SAFETY: each pointer refers to one returned credential.
        let credential = unsafe { &**entry };
        if credential.TargetName.is_null() {
            continue;
        }
        // SAFETY: TargetName is NUL-terminated inside the returned allocation.
        let length =
            (0..).take_while(|&index| unsafe { *credential.TargetName.add(index) } != 0).count();
        // SAFETY: `length` units precede the terminator.
        let target = String::from_utf16_lossy(unsafe {
            std::slice::from_raw_parts(credential.TargetName, length)
        });
        let written = (u64::from(credential.LastWritten.dwHighDateTime) << 32)
            | u64::from(credential.LastWritten.dwLowDateTime);
        times.insert(target, written);
    }
    // SAFETY: the list was allocated by CredEnumerateW and is freed once.
    unsafe { CredFree(list.cast()) };
    times
}

#[test]
fn isolated_renders_leave_the_users_account_credentials_alone() {
    // With its Web Account Manager sign-in on, Edge in the AppContainer made Windows rewrite the
    // user's Microsoft account credentials two or three times per render. Those rewrites are
    // Credential Manager calls that Aura's lock does not order, and Aura's own calls beside them
    // lose updates. Other programs may rewrite them at any moment, so two renders that saw a
    // rewrite are tried once more.
    if account_write_times().is_empty() {
        eprintln!("not_exercised: this user has no Microsoft account credential");
        return;
    }
    let page = "<!doctype html><html><body>static</body></html>";
    let mut seen = Vec::new();
    for _ in 0..2 {
        let before = account_write_times();
        for _ in 0..2 {
            render_fetched_html(page, Duration::from_secs(25)).expect("isolated render");
        }
        let after = account_write_times();
        let rewritten = before.iter().filter(|(target, time)| after.get(*target) != Some(*time));
        let rewritten = rewritten.count();
        if rewritten == 0 {
            return;
        }
        seen.push(rewritten);
    }
    panic!("two isolated renders rewrote the user's account credentials both times: {seen:?}");
}
