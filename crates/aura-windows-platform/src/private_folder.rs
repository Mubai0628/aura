//! Folders only the user, SYSTEM and the administrators can open, kept out of Windows Search:
//! where moved data goes. The access is set as the folder is made, before any file is written
//! into it, and everything made inside inherits it.
use std::{io, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows_sys::Win32::{
    Foundation::{GetLastError, LocalFree},
    Security::{
        ACL,
        Authorization::{
            ConvertSecurityDescriptorToStringSecurityDescriptorW,
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
            SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
        PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES,
    },
    Storage::FileSystem::{
        CreateDirectoryW, FILE_ATTRIBUTE_NOT_CONTENT_INDEXED, GetFileAttributesW,
        INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
    },
};

use crate::security::current_user_sid_string;

/// Full control for the user, SYSTEM and the administrators, inherited by everything inside, and
/// nothing taken from the folders above.
fn private_sddl(user: &str) -> String {
    format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
}

/// Paths this long need the `\\?\` form: a folder's limit is 12 below `MAX_PATH`.
const LONG_PATH_UNITS: usize = 248;

fn wide(path: &Path) -> Vec<u16> {
    let path = if path.as_os_str().encode_wide().count() >= LONG_PATH_UNITS {
        crate::path::extended_length_path(path)
    } else {
        path.to_path_buf()
    };
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// A security descriptor from SDDL, freed when dropped.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    fn from_sddl(sddl: &str) -> io::Result<Self> {
        let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
        let mut descriptor = ptr::null_mut();
        // SAFETY: `text` is NUL-terminated and the output pointer is live.
        let converted = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                text.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        };
        if converted == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(descriptor))
    }

    fn dacl(&self) -> io::Result<*mut ACL> {
        let (mut present, mut dacl, mut defaulted) = (0, ptr::null_mut(), 0);
        // SAFETY: the descriptor is valid for the life of `self`; the outputs are live.
        if unsafe { GetSecurityDescriptorDacl(self.0, &mut present, &mut dacl, &mut defaulted) }
            == 0
        {
            return Err(io::Error::last_os_error());
        }
        if present == 0 || dacl.is_null() {
            return Err(io::Error::other("the descriptor has no DACL"));
        }
        Ok(dacl)
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        // SAFETY: the descriptor was allocated by Windows with LocalAlloc.
        unsafe {
            let _ = LocalFree(self.0.cast());
        }
    }
}

/// Makes `path` (its parent must be there) as a private folder kept out of the index.
pub fn create_private_folder(path: &Path) -> io::Result<()> {
    let descriptor = Descriptor::from_sddl(&private_sddl(&current_user_sid_string()?))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0,
        bInheritHandle: 0,
    };
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated and the attributes point at a live descriptor.
    if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 {
        return Err(io::Error::last_os_error());
    }
    keep_out_of_index(path)
}

/// Gives the existing folder `path` the private access, dropping what it inherited.
pub fn make_folder_private(path: &Path) -> io::Result<()> {
    let descriptor = Descriptor::from_sddl(&private_sddl(&current_user_sid_string()?))?;
    let dacl = descriptor.dacl()?;
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated and `dacl` lives inside `descriptor`.
    let status = unsafe {
        SetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            dacl,
            ptr::null(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    keep_out_of_index(path)
}

/// Adds the not-indexed attribute to a file or folder, keeping its others.
pub fn keep_out_of_index(path: &Path) -> io::Result<()> {
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated.
    let attributes = unsafe { GetFileAttributesW(name.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    if attributes & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED != 0 {
        return Ok(());
    }
    // SAFETY: as above; the attributes are the folder's own plus one.
    if unsafe { SetFileAttributesW(name.as_ptr(), attributes | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// What is wrong with `path` as a private folder, `None` when nothing is: its DACL must be
/// protected and hold exactly the three grants, and it must be kept out of the index.
pub fn private_folder_problem(path: &Path) -> io::Result<Option<String>> {
    let sddl = dacl_sddl(path)?;
    let user = current_user_sid_string()?;
    let Some(rest) = sddl.strip_prefix("D:") else {
        return Ok(Some(format!("unexpected DACL {sddl}")));
    };
    let (flags, aces) = rest.split_at(rest.find('(').unwrap_or(rest.len()));
    if !flags.contains('P') {
        return Ok(Some("the DACL takes grants from the folders above".to_owned()));
    }
    let mut found: Vec<&str> =
        aces.trim_start_matches('(').trim_end_matches(')').split(")(").collect();
    found.sort_unstable();
    let user_ace = format!("A;OICI;FA;;;{user}");
    let mut wanted = vec![user_ace.as_str(), "A;OICI;FA;;;SY", "A;OICI;FA;;;BA"];
    wanted.sort_unstable();
    if found != wanted {
        return Ok(Some(format!("the DACL grants {aces}")));
    }
    let name = wide(path);
    // SAFETY: `name` is NUL-terminated.
    let attributes = unsafe { GetFileAttributesW(name.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    if attributes & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED == 0 {
        return Ok(Some("the folder is indexed by Windows Search".to_owned()));
    }
    Ok(None)
}

/// The DACL of a file or folder as SDDL.
pub fn dacl_sddl(path: &Path) -> io::Result<String> {
    let name = wide(path);
    let mut descriptor = ptr::null_mut();
    // SAFETY: `name` is NUL-terminated and the output pointers are live or null.
    let status = unsafe {
        GetNamedSecurityInfoW(
            name.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut descriptor,
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    let descriptor = Descriptor(descriptor);
    let mut text = ptr::null_mut();
    // SAFETY: the descriptor came from GetNamedSecurityInfoW; `text` is a live output.
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor.0,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            ptr::null_mut(),
        )
    };
    if converted == 0 {
        // SAFETY: reads this thread's last error.
        return Err(io::Error::from_raw_os_error(unsafe { GetLastError() } as i32));
    }
    let mut length = 0;
    // SAFETY: Windows wrote a NUL-terminated string at `text`.
    while unsafe { *text.add(length) } != 0 {
        length += 1;
    }
    // SAFETY: `length` units were just read from the string.
    let value = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(text, length) });
    // SAFETY: the string was allocated by Windows with LocalAlloc.
    unsafe {
        let _ = LocalFree(text.cast());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_private_folder_grants_only_the_user_system_and_administrators_and_its_files_inherit() {
        let temp = tempfile::tempdir().expect("TEMP");
        let folder = temp.path().join("moved");
        create_private_folder(&folder).expect("create");
        assert_eq!(private_folder_problem(&folder).expect("read"), None);
        std::fs::write(folder.join("file.txt"), b"x").expect("write");
        let inherited = dacl_sddl(&folder.join("file.txt")).expect("file DACL");
        assert!(!inherited.contains("WD") && !inherited.contains(";BU)"), "{inherited}");
        assert!(inherited.contains("ID;FA;;;SY"), "{inherited}");

        let plain = temp.path().join("plain");
        std::fs::create_dir(&plain).expect("plain folder");
        assert!(private_folder_problem(&plain).expect("read").is_some());
        make_folder_private(&plain).expect("make private");
        assert_eq!(private_folder_problem(&plain).expect("read"), None);
    }
}
