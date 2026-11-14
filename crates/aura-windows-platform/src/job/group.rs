//! Group Jobs: execution Jobs are nested in them at creation, so the limits a group holds apply
//! to every process of its nested Jobs together. Closing a group ends none of its processes;
//! each nested execution Job keeps its own end.
use super::*;
use windows_sys::Win32::System::JobObjects::{
    JOB_OBJECT_CPU_RATE_CONTROL_ENABLE, JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
    JOB_OBJECT_LIMIT_JOB_MEMORY, JOBOBJECT_CPU_RATE_CONTROL_INFORMATION,
    JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0, JOBOBJECTINFOCLASS,
    JobObjectCpuRateControlInformation,
};

/// What the processes of one group may use together.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GroupLimits {
    /// Hard cap on the share of all processors' time, in hundredths of a percent (1..=10000).
    pub cpu_rate: u32,
    /// Memory the group's processes may commit together; `None` sets no memory cap.
    pub memory_bytes: Option<u64>,
}

/// A Job that execution Jobs are nested in, never one a process is created in alone.
pub struct GroupJob {
    handle: OwnedHandle,
}

impl GroupJob {
    pub fn create(limits: GroupLimits) -> io::Result<Self> {
        check_rate(limits.cpu_rate)?;
        // SAFETY: null security/name pointers request a private, non-inheritable unnamed job.
        let handle = OwnedHandle::from_raw(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        // SAFETY: `handle` is valid and the flags arguments are documented.
        if unsafe { SetHandleInformation(handle.as_raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if let Some(bytes) = limits.memory_bytes {
            let mut memory = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            memory.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_JOB_MEMORY;
            memory.JobMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
            set(&handle, JobObjectExtendedLimitInformation, &memory)?;
        }
        let group = Self { handle };
        group.set_cpu_rate(limits.cpu_rate)?;
        Ok(group)
    }

    /// Sets the group's hard CPU cap; the processes already in it are held to the new one.
    pub fn set_cpu_rate(&self, cpu_rate: u32) -> io::Result<()> {
        check_rate(cpu_rate)?;
        let rate = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
            ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
            Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 { CpuRate: cpu_rate },
        };
        set(&self.handle, JobObjectCpuRateControlInformation, &rate)
    }

    /// The limits Windows holds for the group.
    pub fn limits(&self) -> io::Result<GroupLimits> {
        let mut rate = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
        query(&self.handle, JobObjectCpuRateControlInformation, &mut rate)?;
        let mut memory = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        query(&self.handle, JobObjectExtendedLimitInformation, &mut memory)?;
        let hard = JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP;
        Ok(GroupLimits {
            // SAFETY: a hard-capped rate is the union's `CpuRate` member.
            cpu_rate: if rate.ControlFlags & hard == hard {
                unsafe { rate.Anonymous.CpuRate }
            } else {
                0
            },
            memory_bytes: (memory.BasicLimitInformation.LimitFlags & JOB_OBJECT_LIMIT_JOB_MEMORY
                != 0)
                .then_some(memory.JobMemoryLimit as u64),
        })
    }

    /// Processes alive in the group's nested Jobs.
    pub fn active_processes(&self) -> io::Result<u32> {
        job_active_processes(&self.handle)
    }

    pub fn contains_identity(&self, expected: &ProcessIdentity) -> io::Result<bool> {
        RootJob::contains_in(&self.handle, expected)
    }

    pub(crate) fn duplicate_for_launch(&self) -> io::Result<OwnedHandle> {
        RootJob::duplicate_handle(&self.handle)
    }
}

fn check_rate(cpu_rate: u32) -> io::Result<()> {
    if (1..=10_000).contains(&cpu_rate) {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "group CPU rate out of range"))
    }
}

fn set<T>(handle: &OwnedHandle, class: JOBOBJECTINFOCLASS, value: &T) -> io::Result<()> {
    let size = u32::try_from(size_of::<T>())
        .map_err(|_| io::Error::other("job information size overflow"))?;
    // SAFETY: `value` is the typed record of `class`, live for the call; the Job is owned.
    if unsafe { SetInformationJobObject(handle.as_raw(), class, (&raw const *value).cast(), size) }
        == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn query<T>(handle: &OwnedHandle, class: JOBOBJECTINFOCLASS, value: &mut T) -> io::Result<()> {
    let size = u32::try_from(size_of::<T>())
        .map_err(|_| io::Error::other("job information size overflow"))?;
    // SAFETY: `value` is the writable typed record of `class`; the Job is owned.
    if unsafe {
        QueryInformationJobObject(
            handle.as_raw(),
            class,
            (&raw mut *value).cast(),
            size,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
#[path = "group_tests.rs"]
mod tests;
