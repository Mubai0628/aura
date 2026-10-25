use std::{ffi::OsStr, io, os::windows::ffi::OsStrExt, ptr};

use tokio::net::windows::named_pipe::{NamedPipeClient, NamedPipeServer, PipeMode, ServerOptions};
use windows_sys::Win32::{
    Foundation::INVALID_HANDLE_VALUE,
    Storage::FileSystem::{
        CreateFileW, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, FILE_READ_DATA,
        FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, OPEN_EXISTING, SECURITY_IDENTIFICATION,
        SECURITY_SQOS_PRESENT,
    },
};

use crate::security::ObjectSecurityDescriptor;

/// Creates Aura's single-instance, local-only byte-mode pipe with a custom
/// DACL for the current Logon SID and LOCAL_SYSTEM.
pub fn create_secure_byte_pipe_server(
    pipe_name: &str,
    first_pipe_instance: bool,
) -> io::Result<NamedPipeServer> {
    let mut security = ObjectSecurityDescriptor::for_current_logon()?;
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .first_pipe_instance(first_pipe_instance)
        .max_instances(1)
        .reject_remote_clients(true);
    // SAFETY: `security` owns a valid SECURITY_ATTRIBUTES and security
    // descriptor for the duration of CreateNamedPipeW. Windows copies the
    // descriptor into the new kernel object before this call returns.
    unsafe { options.create_with_security_attributes_raw(pipe_name, security.attributes_ptr()) }
}

/// Dedicated Runner endpoint, separate from Main/Core and Secret channels.
/// Only the exact launched process is accepted after connection; this ACL is
/// necessary for cross-account access but is not the authentication decision.
#[cfg(feature = "restricted-user-runtime")]
pub fn create_execution_byte_pipe_server(
    pipe_name: &str,
    account_sid: &str,
) -> io::Result<NamedPipeServer> {
    let mut security = ObjectSecurityDescriptor::for_execution_account(account_sid)?;
    let mut options = ServerOptions::new();
    options
        .pipe_mode(PipeMode::Byte)
        .first_pipe_instance(true)
        .max_instances(1)
        .reject_remote_clients(true);
    // SAFETY: retained native descriptor is copied by CreateNamedPipeW.
    unsafe { options.create_with_security_attributes_raw(pipe_name, security.attributes_ptr()) }
}

/// Opens Aura's duplex byte-mode pipe without requesting either generic write
/// access or `FILE_CREATE_PIPE_INSTANCE` (the two overlap for named pipes).
pub fn open_secure_byte_pipe_client(pipe_name: &str) -> io::Result<NamedPipeClient> {
    let encoded = OsStr::new(pipe_name).encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
    if encoded[..encoded.len().saturating_sub(1)].contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "pipe name contains NUL"));
    }
    let desired_access =
        FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES;
    let flags = FILE_FLAG_OVERLAPPED | SECURITY_IDENTIFICATION | SECURITY_SQOS_PRESENT;
    // SAFETY: `encoded` is NUL-terminated and remains live for the call. The
    // returned unique handle is transferred exactly once to Tokio.
    let handle = unsafe {
        CreateFileW(
            encoded.as_ptr(),
            desired_access,
            0,
            ptr::null(),
            OPEN_EXISTING,
            flags,
            ptr::null_mut(),
        )
    };
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned one owned overlapped named-pipe handle, and
    // Tokio becomes its sole owner even when registration returns an error.
    unsafe { NamedPipeClient::from_raw_handle(handle.cast()) }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    use super::{create_secure_byte_pipe_server, open_secure_byte_pipe_client};

    #[tokio::test]
    async fn minimal_access_client_roundtrips_through_real_dacl() {
        let mut random = [0_u8; 8];
        getrandom::fill(&mut random).expect("random pipe suffix");
        let suffix = u64::from_le_bytes(random);
        let pipe_name = format!(r"\\.\pipe\aura-p04-dacl-{}-{suffix:016x}", std::process::id());
        let mut server =
            create_secure_byte_pipe_server(&pipe_name, true).expect("secure pipe server");
        let mut client = open_secure_byte_pipe_client(&pipe_name).expect("minimal client open");
        server.connect().await.expect("server connect");

        client.write_all(b"ping").await.expect("client write");
        let mut request = [0_u8; 4];
        server.read_exact(&mut request).await.expect("server read");
        assert_eq!(&request, b"ping");

        server.write_all(b"pong").await.expect("server write");
        let mut response = [0_u8; 4];
        client.read_exact(&mut response).await.expect("client read");
        assert_eq!(&response, b"pong");
    }
}
