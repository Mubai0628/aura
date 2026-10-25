//! Read the current user's Windows environment anew, without inheriting Core's secrets.
use super::*;
pub(super) fn current_process_token(access: u32) -> io::Result<OwnedHandle> {
    let mut token = ptr::null_mut();
    // SAFETY: the pseudo-handle is valid; output receives a new owned handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), access, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    OwnedHandle::from_raw(token)
}
use std::{ffi::OsString, os::windows::ffi::OsStringExt};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};

pub fn current_user_environment() -> io::Result<Vec<(OsString, OsString)>> {
    let token = current_process_token(TOKEN_QUERY | TOKEN_DUPLICATE)?;
    let mut raw = ptr::null_mut();
    // SAFETY: token is retained and raw receives a system-owned double-NUL-terminated block.
    if unsafe { CreateEnvironmentBlock(&mut raw, token.as_raw(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    struct Block(*mut std::ffi::c_void);
    impl Drop for Block {
        fn drop(&mut self) {
            // SAFETY: this block came from CreateEnvironmentBlock and is freed once.
            unsafe {
                DestroyEnvironmentBlock(self.0);
            }
        }
    }
    let block = Block(raw);
    let mut cursor = block.0.cast::<u16>();
    let mut entries = Vec::new();
    // SAFETY: Windows guarantees terminated UTF-16 entries and a final empty entry.
    unsafe {
        while *cursor != 0 {
            let mut length = 0;
            while *cursor.add(length) != 0 {
                length += 1;
            }
            let line = std::slice::from_raw_parts(cursor, length);
            if let Some(equal) = line.iter().position(|unit| *unit == u16::from(b'='))
                && equal > 0
            {
                entries.push((
                    OsString::from_wide(&line[..equal]),
                    OsString::from_wide(&line[equal + 1..]),
                ));
            }
            cursor = cursor.add(length + 1);
        }
    }
    Ok(entries)
}
