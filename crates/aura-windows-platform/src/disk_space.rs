//! Free space on the volume a path is on, as the current user may use it (quotas included).
use std::{io, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

pub fn free_disk_space(path: &Path) -> io::Result<u64> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut available = 0_u64;
    // SAFETY: `wide` is NUL-terminated and outlives the call; the out pointer is a live u64.
    let succeeded = unsafe {
        GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, ptr::null_mut(), ptr::null_mut())
    };
    if succeeded == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(available)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_temporary_folder_has_some_free_space_and_a_missing_path_fails() {
        assert!(free_disk_space(&std::env::temp_dir()).expect("free space") > 0);
        assert!(free_disk_space(Path::new(r"Q:\definitely\missing\path")).is_err());
    }
}
