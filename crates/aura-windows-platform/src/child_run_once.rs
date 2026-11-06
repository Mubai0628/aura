//! A temporary startup value for one child-session opening. Cleanup removes only the value
//! this instance wrote, never the key or another opening's value.
use std::{io, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
    System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegGetValueW, RegSetValueExW,
    },
};

const RUN_ONCE: &str = r"Software\Microsoft\Windows\CurrentVersion\RunOnce";
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}
fn check(status: u32) -> io::Result<()> {
    if status == ERROR_SUCCESS { Ok(()) } else { Err(io::Error::from_raw_os_error(status as i32)) }
}
struct Key(HKEY);
impl Drop for Key {
    fn drop(&mut self) {
        // SAFETY: a handle opened here, closed once.
        unsafe {
            RegCloseKey(self.0);
        }
    }
}
fn open(path: &str) -> io::Result<Key> {
    let mut key = ptr::null_mut();
    // SAFETY: predefined current-user root, live buffers, writable handle storage.
    check(unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            wide(path).as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE | KEY_SET_VALUE,
            ptr::null(),
            &raw mut key,
            ptr::null_mut(),
        )
    })?;
    Ok(Key(key))
}
fn read(key: &Key, name: &str) -> io::Result<Option<String>> {
    let mut buffer = [0_u16; 4096];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: the open key and writable buffer live for the call.
    let status = unsafe {
        RegGetValueW(
            key.0,
            ptr::null(),
            wide(name).as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &raw mut bytes,
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(status)?;
    let value = &buffer[..(bytes as usize / 2).min(buffer.len())];
    Ok(Some(String::from_utf16_lossy(value.strip_suffix(&[0]).unwrap_or(value))))
}
fn write(key: &Key, name: &str, command: &str) -> io::Result<()> {
    let value = wide(command);
    // SAFETY: the key is writable; the string is a live NUL-terminated UTF-16 buffer.
    check(unsafe {
        RegSetValueExW(
            key.0,
            wide(name).as_ptr(),
            0,
            REG_SZ,
            value.as_ptr().cast(),
            (value.len() * 2) as u32,
        )
    })
}

/// Owns the exact temporary value. Dropping it after success, failure or cancellation cleans up.
pub struct ChildRunOnce {
    path: String,
    name: String,
    command: String,
    armed: bool,
}
impl ChildRunOnce {
    pub fn install(name: &str, command: &str) -> io::Result<Self> {
        Self::install_at(RUN_ONCE, name, command)
    }
    fn install_at(path: &str, name: &str, command: &str) -> io::Result<Self> {
        if !name.starts_with("AuraChild-")
            || name.contains(['\0', '\\', '/'])
            || name.len() > 128
            || command.contains('\0')
            || command.encode_utf16().count() > 260
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid temporary child startup value",
            ));
        }
        let key = open(path)?;
        if read(&key, name)?.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "temporary startup name is already used",
            ));
        }
        write(&key, name, command)?;
        Ok(Self {
            path: path.to_owned(),
            name: name.to_owned(),
            command: command.to_owned(),
            armed: true,
        })
    }
    pub fn remove(&mut self) -> io::Result<()> {
        if !self.armed {
            return Ok(());
        }
        let key = open(&self.path)?;
        match read(&key, &self.name)? {
            None => {}
            Some(current) if current == self.command => {
                // SAFETY: remove this exact value only; the key and all other values remain.
                let status = unsafe { RegDeleteValueW(key.0, wide(&self.name).as_ptr()) };
                if status != ERROR_FILE_NOT_FOUND {
                    check(status)?;
                }
            }
            Some(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "temporary startup value changed; retained",
                ));
            }
        }
        self.armed = false;
        Ok(())
    }
}
impl Drop for ChildRunOnce {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

#[cfg(test)]
#[path = "child_run_once_tests.rs"]
mod tests;
