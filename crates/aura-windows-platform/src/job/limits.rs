//! Single-process jobs for fixed native reads, and the memory and process limits of a plugin's
//! process trees. Ordinary execution jobs retain their tree policy.
use super::*;
use windows_sys::Win32::System::JobObjects::{
    JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_JOB_MEMORY,
};

/// What every process in one Job may use together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JobLimits {
    pub memory_bytes: u64,
    /// Processes alive at once.
    pub active_processes: u32,
}

impl RootJob {
    fn extended_limits(&self) -> io::Result<JOBOBJECT_EXTENDED_LIMIT_INFORMATION> {
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        // SAFETY: query the exact owned Job into a typed writable limit record.
        if unsafe {
            QueryInformationJobObject(
                self.handle.as_raw(),
                JobObjectExtendedLimitInformation,
                (&raw mut limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(limits)
    }

    /// Adds `limits` to the Job's own (the tree still ends when the Job closes). Set before any
    /// process joins it.
    pub fn limit(&self, limits: JobLimits) -> io::Result<()> {
        let mut current = self.extended_limits()?;
        current.BasicLimitInformation.LimitFlags |=
            JOB_OBJECT_LIMIT_JOB_MEMORY | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        current.BasicLimitInformation.ActiveProcessLimit = limits.active_processes;
        current.JobMemoryLimit = usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX);
        // SAFETY: this live Job is owned; the typed limit record is exact.
        if unsafe {
            SetInformationJobObject(
                self.handle.as_raw(),
                JobObjectExtendedLimitInformation,
                (&raw const current).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The most memory the Job's processes committed together so far, in bytes.
    pub fn peak_memory(&self) -> io::Result<u64> {
        Ok(self.extended_limits()?.PeakJobMemoryUsed as u64)
    }

    pub(crate) fn verify_single_process(&self) -> io::Result<()> {
        let limits = self.extended_limits()?;
        let flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        if limits.BasicLimitInformation.LimitFlags != flags
            || limits.BasicLimitInformation.ActiveProcessLimit != 1
        {
            return Err(io::Error::other("git.single_process_job_required"));
        }
        Ok(())
    }
    pub fn create_single_process() -> io::Result<Self> {
        let job = Self::create()?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        // SAFETY: this private live Job has no members yet; the typed limit record is exact.
        if unsafe {
            SetInformationJobObject(
                job.handle.as_raw(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }
}
