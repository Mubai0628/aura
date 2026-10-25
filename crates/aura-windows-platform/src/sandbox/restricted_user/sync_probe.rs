//! Fixed fresh synchronization objects owned only by this diagnostic process.
use crate::handle::OwnedHandle;
use serde_json::{Value, json};
use std::{io, ptr};
use windows_sys::Win32::System::Threading::{
    CreateEventW, CreateMutexW, CreateSemaphoreW, ReleaseMutex, ReleaseSemaphore, SetEvent,
    WaitForSingleObject,
};

pub fn spike_synchronization() -> Value {
    fn outcome(ok: bool) -> Value {
        if ok {
            json!({"ok":true})
        } else {
            json!({"ok":false,"os_error":io::Error::last_os_error().raw_os_error()})
        }
    }
    let mut rows = Vec::new();
    for (kind, named) in [
        ("event", false),
        ("semaphore", false),
        ("mutex", false),
        ("event", true),
        ("semaphore", true),
        ("mutex", true),
    ] {
        let mut entropy = [0u8; 16];
        if let Err(error) = getrandom::fill(&mut entropy) {
            rows.push(json!({"kind":kind,"named":named,"entropy_error":error.to_string()}));
            continue;
        }
        let name = format!(
            "Local\\AuraR06Sync{}",
            entropy.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
        let name_ptr = if named { name.as_ptr() } else { ptr::null() };
        // SAFETY: unnamed or fresh random-name objects with the process default DACL;
        // null optional pointers; a collision is detected before any operation on the returned handle.
        let raw = unsafe {
            match kind {
                "event" => CreateEventW(ptr::null(), 0, 0, name_ptr),
                "semaphore" => CreateSemaphoreW(ptr::null(), 0, 1, name_ptr),
                _ => CreateMutexW(ptr::null(), 0, name_ptr),
            }
        };
        let create_error = io::Error::last_os_error();
        let handle = match OwnedHandle::from_raw(raw) {
            Ok(value) => value,
            Err(error) => {
                rows.push(json!({"kind":kind,"named":named,"created":false,"os_error":error.raw_os_error()}));
                continue;
            }
        };
        if named && create_error.raw_os_error() == Some(183) {
            rows.push(json!({"kind":kind,"named":named,"collision":true}));
            continue;
        }
        // SAFETY: retained object of the matching kind, no wait beyond a poll.
        let signal = unsafe {
            match kind {
                "event" => outcome(SetEvent(handle.as_raw()) != 0),
                "semaphore" => outcome(ReleaseSemaphore(handle.as_raw(), 1, ptr::null_mut()) != 0),
                _ => json!({"ok":true}),
            }
        };
        let wait = unsafe { WaitForSingleObject(handle.as_raw(), 0) };
        let waited = outcome(wait == 0);
        if kind == "mutex" && wait == 0 {
            unsafe {
                ReleaseMutex(handle.as_raw());
            }
        }
        rows.push(
            json!({"kind":kind,"named":named,"created":true,"signal":signal,"wait":waited,"wait_code":wait}),
        );
    }
    json!(rows)
}
