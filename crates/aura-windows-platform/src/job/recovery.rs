//! Owner-only named outer Jobs make post-crash process quiescence observable.
//! They are transient execution resources, with no service/control endpoint.
use super::*;
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_NOT_FOUND, SetLastError},
    System::JobObjects::{OpenJobObjectW, QueryInformationJobObject},
};

fn name(installation: &str, nonce: &str) -> io::Result<String> {
    let uuid = installation.len() == 36
        && installation.bytes().enumerate().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit() && !c.is_ascii_uppercase()
            }
        });
    if !uuid
        || nonce.len() != 32
        || !nonce.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(io::Error::other("invalid execution Job binding"));
    }
    Ok(format!(r"Global\Aura.Execution.{installation}.{nonce}"))
}

impl RootJob {
    /// Call only after the private journal has durably recorded this binding,
    /// and before any Runner has been launched. Existing objects are not adopted.
    pub fn create_recoverable(installation: &str, nonce: &str) -> io::Result<Self> {
        let name = wide_null(&name(installation, nonce)?);
        let mut security = crate::security::ObjectSecurityDescriptor::for_execution_job()?;
        // SAFETY: exact descriptor/name buffers remain live; no inherited handle.
        let (raw, status) = unsafe {
            SetLastError(0);
            let raw = CreateJobObjectW(security.attributes_ptr().cast(), name.as_ptr());
            (raw, GetLastError())
        };
        let handle = OwnedHandle::from_raw(raw)?;
        if status == ERROR_ALREADY_EXISTS {
            return Err(io::Error::other("execution Job already exists; no adoption"));
        }
        Self::configure(handle)
    }

    /// Caller must hold the installation and execution exclusion locks. Only
    /// the recorded owner Job is reopened; access failures never mean stopped.
    pub fn open_for_recovery(installation: &str, nonce: &str) -> io::Result<Option<Self>> {
        let name = wide_null(&name(installation, nonce)?);
        // READ_CONTROL plus only query/termination, never assign or change limits.
        let raw = unsafe {
            // Winnt.h: READ_CONTROL 0x20000, QUERY 0x4, TERMINATE 0x8.
            OpenJobObjectW(0x0002_000c, 0, name.as_ptr())
        };
        if raw.is_null() {
            let status = unsafe { GetLastError() };
            return if status == ERROR_FILE_NOT_FOUND {
                Ok(None)
            } else {
                Err(io::Error::from_raw_os_error(status as i32))
            };
        }
        let handle = OwnedHandle::from_raw(raw)?;
        crate::security::verify_execution_job_owner(handle.as_raw())?;
        let job = Self { handle };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        if unsafe {
            QueryInformationJobObject(
                job.handle.as_raw(),
                JobObjectExtendedLimitInformation,
                (&raw mut limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // A crash before limits are set can leave an empty, never-used object.
        // An active object must still have the exact non-breakaway outer limits.
        if limits.BasicLimitInformation.LimitFlags != JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            && job.active_processes()? != 0
        {
            return Err(io::Error::other("execution Job limits changed"));
        }
        Ok(Some(job))
    }

    pub fn stop_for_recovery(&self) -> io::Result<()> {
        self.terminate_tree()?;
        for _ in 0..100 {
            if self.active_processes()? == 0 {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err(io::Error::other("execution Job termination unconfirmed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const INSTALL: &str = "b3000000-0000-4000-8000-000000000001";
    fn nonce() -> String {
        let mut value = [0; 16];
        getrandom::fill(&mut value).expect("random fixture");
        value.iter().map(|v| format!("{v:02x}")).collect()
    }
    #[test]
    fn recovery_job_rejects_name_injection_and_does_not_adopt_existing_object() {
        assert!(RootJob::create_recoverable(INSTALL, r"..\unrelated").is_err());
        assert!(RootJob::open_for_recovery("not-an-installation", &nonce()).is_err());
        let nonce = nonce();
        assert!(RootJob::open_for_recovery(INSTALL, &nonce).expect("absent").is_none());
        let job = RootJob::create_recoverable(INSTALL, &nonce).expect("owner-only job");
        assert!(RootJob::create_recoverable(INSTALL, &nonce).is_err());
        let reopened = RootJob::open_for_recovery(INSTALL, &nonce).expect("open").expect("exists");
        reopened.stop_for_recovery().expect("confirmed empty");
        drop(reopened);
        assert_eq!(job.active_processes().expect("unchanged owned job"), 0);
        drop(job);
        assert!(RootJob::open_for_recovery(INSTALL, &nonce).expect("destroyed").is_none());
    }

    #[test]
    fn recovery_job_observes_and_stops_a_real_process_before_returning() {
        use std::os::windows::{io::AsRawHandle, process::CommandExt};
        let nonce = nonce();
        let job = RootJob::create_recoverable(INSTALL, &nonce).expect("job");
        // A native CMD waiting on this test's pipe, with no file/network action.
        let mut child = crate::test_spawn::spawn(
            std::process::Command::new(
                crate::windows_directory().expect("Windows").join("System32/cmd.exe"),
            )
            .args(["/d", "/c", "set /p AURA_JOB_FIXTURE="])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .creation_flags(0x0800_0000),
        )
        .expect("bounded CMD fixture");
        // SAFETY: both handles are live; this test owns the harmless process.
        let assigned =
            unsafe { AssignProcessToJobObject(job.handle.as_raw(), child.as_raw_handle().cast()) };
        if assigned == 0 {
            let _ = child.kill();
            let _ = child.wait();
            panic!("assign fixture: {}", io::Error::last_os_error());
        }
        assert_eq!(job.active_processes().expect("active"), 1);
        let reopened = RootJob::open_for_recovery(INSTALL, &nonce).expect("open").expect("exists");
        reopened.stop_for_recovery().expect("observed stop");
        assert_eq!(job.active_processes().expect("empty"), 0);
        assert!(!child.wait().expect("actual exit").success());
    }
}
