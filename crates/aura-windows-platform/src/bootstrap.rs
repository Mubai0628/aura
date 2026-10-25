use std::{io, ptr};

use windows_sys::Win32::{Foundation::HANDLE, Storage::FileSystem::ReadFile};

use crate::handle::OwnedHandle;

pub const BOOTSTRAP_SECRET_BYTES: usize = 32;

/// Consumes the anonymous-pipe handle explicitly inherited by Core. The raw
/// handle value is only a locator; the 256-bit secret itself never appears in
/// argv, the environment, or logs.
pub fn read_inherited_bootstrap_secret(
    inherited_handle_value: usize,
) -> io::Result<[u8; BOOTSTRAP_SECRET_BYTES]> {
    if inherited_handle_value == 0 || inherited_handle_value == usize::MAX {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "bootstrap handle is invalid"));
    }
    let raw = inherited_handle_value as HANDLE;
    let handle = OwnedHandle::from_raw(raw)?;
    let mut secret = [0_u8; BOOTSTRAP_SECRET_BYTES];
    let mut offset = 0_usize;
    while offset < secret.len() {
        let mut read = 0_u32;
        // SAFETY: `handle` uniquely owns the inherited anonymous-pipe endpoint
        // and the remaining secret slice is valid writable storage.
        if unsafe {
            ReadFile(
                handle.as_raw(),
                secret[offset..].as_mut_ptr(),
                u32::try_from(secret.len() - offset)
                    .map_err(|_| io::Error::other("bootstrap secret length overflow"))?,
                &mut read,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "bootstrap secret pipe closed early",
            ));
        }
        offset += usize::try_from(read)
            .map_err(|_| io::Error::other("bootstrap read length overflow"))?;
    }
    drop(handle);
    Ok(secret)
}
