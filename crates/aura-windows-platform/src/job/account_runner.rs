//! Credentialed, suspended Runner launch. No shell, environment inheritance,
//! elevation fallback or unbound executable search is permitted here.
use super::*;
use crate::{NativeEnvironmentEntry, SecretBytes};
use windows_sys::Win32::{
    System::Threading::{
        CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessWithLogonW, LOGON_WITH_PROFILE,
        STARTF_USESHOWWINDOW,
    },
    UI::WindowsAndMessaging::SW_HIDE,
};
use zeroize::Zeroizing;

pub struct AccountRunnerLaunch<'a> {
    pub account_name: &'a str,
    pub password: &'a SecretBytes,
    pub executable: &'a Path,
    pub arguments: &'a [String],
    pub directory: &'a Path,
    pub environment: &'a [NativeEnvironmentEntry],
}

impl RootJob {
    pub fn terminate_tree(&self) -> io::Result<()> {
        if unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.handle.as_raw(), 1)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub fn active_processes(&self) -> io::Result<u32> {
        use windows_sys::Win32::System::JobObjects::*;
        let mut value = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        if unsafe {
            QueryInformationJobObject(
                self.handle.as_raw(),
                JobObjectBasicAccountingInformation,
                (&raw mut value).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(value.ActiveProcesses)
    }
    pub fn spawn_account_runner(
        &self,
        launch: AccountRunnerLaunch<'_>,
    ) -> io::Result<ManagedProcess> {
        if crate::current_process_is_elevated()?
            || launch.account_name.is_empty()
            || launch.account_name.contains(['\\', '/', '@', '\0'])
            || !launch.executable.is_absolute()
            || !launch.directory.is_absolute()
            || launch.arguments.iter().any(|v| v.contains('\0'))
        {
            return Err(io::Error::other("invalid or elevated account Runner launch"));
        }
        let account = wide_null(launch.account_name);
        let domain = wide_null(".");
        let secret = std::str::from_utf8(launch.password.expose())
            .map_err(|_| io::Error::other("invalid account secret encoding"))?;
        if secret.is_empty() || secret.contains('\0') {
            return Err(io::Error::other("invalid account secret"));
        }
        let password = Zeroizing::new(wide_null(secret));
        let application = wide_null(&launch.executable.to_string_lossy());
        let line = std::iter::once(launch.executable.to_string_lossy().into_owned())
            .chain(launch.arguments.iter().cloned())
            .map(|v| quote_windows_argument(&v))
            .collect::<Vec<_>>()
            .join(" ");
        let mut line = wide_null(&line);
        // CreateProcessWithLogonW has a smaller limit than CreateProcessW.
        if line.len() > 1024 {
            return Err(io::Error::other("Runner bootstrap command line exceeds Windows limit"));
        }
        let directory = wide_null(&launch.directory.to_string_lossy());
        let environment =
            crate::sandbox::environment_block(launch.environment, None, launch.directory)?;
        let startup = STARTUPINFOW {
            cb: size_of::<STARTUPINFOW>() as u32,
            dwFlags: STARTF_USESHOWWINDOW,
            wShowWindow: SW_HIDE as u16,
            ..Default::default()
        };
        let mut information = PROCESS_INFORMATION::default();
        // SAFETY: local credential and all strings/buffers stay live for the
        // call. No standard handles or bootstrap secrets are inherited. This
        // uses a real logon, never LOGON_NETCREDENTIALS_ONLY.
        if unsafe {
            CreateProcessWithLogonW(
                account.as_ptr(),
                domain.as_ptr(),
                password.as_ptr(),
                LOGON_WITH_PROFILE,
                application.as_ptr(),
                line.as_mut_ptr(),
                CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT,
                environment.as_ptr().cast(),
                directory.as_ptr(),
                &startup,
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process = OwnedHandle::from_raw(information.hProcess)?;
        let thread = OwnedHandle::from_raw(information.hThread)?;
        // The account cannot execute code until ownership by Core's Job is
        // established. Errors terminate the still-suspended process.
        if unsafe { AssignProcessToJobObject(self.handle.as_raw(), process.as_raw()) } == 0 {
            let error = io::Error::last_os_error();
            unsafe {
                TerminateProcess(process.as_raw(), 1);
            }
            return Err(error);
        }
        if unsafe { ResumeThread(thread.as_raw()) } == u32::MAX {
            let error = io::Error::last_os_error();
            unsafe {
                TerminateProcess(process.as_raw(), 1);
            }
            return Err(error);
        }
        Ok(ManagedProcess {
            process,
            process_id: information.dwProcessId,
            executable: launch.executable.into(),
        })
    }
}
