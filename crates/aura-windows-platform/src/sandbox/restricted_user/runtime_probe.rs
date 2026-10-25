//! Fixed system DLL startup diagnostic. No host configuration or ACL changes.
use serde_json::{Value, json};
use windows_sys::Win32::{
    Foundation::FreeLibrary,
    System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
};

pub fn spike_runtime_dlls() -> Value {
    let mut rows = Vec::new();
    for name in [
        "bcrypt.dll",
        "bcryptprimitives.dll",
        "ncrypt.dll",
        "mscoree.dll",
        "ucrtbase.dll",
        "user32.dll",
        "gdi32.dll",
    ] {
        let wide = name.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        // SAFETY: fixed system-only names; no caller supplied DLL/path.
        let module = unsafe {
            LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
        };
        if module.is_null() {
            rows.push(json!({"dll":name,"loaded":false,"os_error":std::io::Error::last_os_error().raw_os_error()}));
        } else {
            rows.push(json!({"dll":name,"loaded":true}));
            // SAFETY: balances exactly this successful LoadLibrary reference.
            unsafe {
                FreeLibrary(module);
            }
        }
    }
    json!(rows)
}
