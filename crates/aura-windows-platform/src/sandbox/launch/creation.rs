//! Shared Win32 creation. Token selection and startup policy belong to the caller.
use super::*;

pub(crate) struct NativeStartup<'a> {
    pub application: &'a [u16],
    pub command_line: &'a mut [u16],
    pub directory: &'a [u16],
    pub environment: &'a [u16],
    pub startup: &'a STARTUPINFOEXW,
    pub token: Option<&'a OwnedHandle>,
    pub as_user: bool,
    pub inherit_handles: bool,
    pub flags: u32,
}

pub(crate) fn create_native_process(spec: NativeStartup<'_>) -> io::Result<PROCESS_INFORMATION> {
    let mut information = PROCESS_INFORMATION::default();
    // SAFETY: buffers and startup attributes are live; command_line is mutable.
    // Inheritance and token authority were selected by the audited caller.
    let created = unsafe {
        if spec.token.is_some() || spec.as_user {
            CreateProcessAsUserW(
                spec.token.map_or(ptr::null_mut(), OwnedHandle::as_raw),
                spec.application.as_ptr(),
                spec.command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                i32::from(spec.inherit_handles),
                spec.flags,
                spec.environment.as_ptr().cast(),
                spec.directory.as_ptr(),
                &raw const spec.startup.StartupInfo,
                &mut information,
            )
        } else {
            CreateProcessW(
                spec.application.as_ptr(),
                spec.command_line.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                i32::from(spec.inherit_handles),
                spec.flags,
                spec.environment.as_ptr().cast(),
                spec.directory.as_ptr(),
                &raw const spec.startup.StartupInfo,
                &mut information,
            )
        }
    };
    if created == 0 { Err(io::Error::last_os_error()) } else { Ok(information) }
}
