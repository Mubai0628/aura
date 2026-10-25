//! One suspended-root adoption path for pipes and ConPTY.
use super::*;

pub(crate) fn adopt_created_process(
    job: &OwnedHandle,
    information: PROCESS_INFORMATION,
    overrides: &mut LaunchOverrides<'_>,
) -> io::Result<OwnedHandle> {
    let process = OwnedHandle::from_raw(information.hProcess)?;
    let thread = OwnedHandle::from_raw(information.hThread)?;
    if let Some((trace, origin)) = overrides.trace.as_mut() {
        trace.root_created = true;
        trace.created = origin.elapsed();
    }
    let mut member = 0;
    if overrides.job_at_creation || overrides.trace.is_some() {
        // SAFETY: exact owned handles; this runs before any explicit assignment or resume.
        if unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                process.as_raw(),
                job.as_raw(),
                &mut member,
            )
        } == 0
        {
            let error = last_os_error_with_context("query root Job membership after creation");
            stop_unstarted(&process, &mut overrides.trace);
            return Err(error);
        }
    }
    if let Some((trace, _)) = overrides.trace.as_mut() {
        trace.created_in_job = member != 0;
        match crate::process::snapshot_process_handle(process.as_raw(), information.dwProcessId) {
            Ok(identity) => trace.root_identity = Some(identity),
            Err(error) => {
                stop_unstarted(&process, &mut overrides.trace);
                return Err(error);
            }
        }
    }
    // The compatibility fault name is retained as a pre-resume failure seam. Compat
    // Shell and ConPTY already belong to the Job; they must never assign after creation.
    #[cfg(any(test, feature = "compat-adoption"))]
    let assignment_fault = overrides.fault == Some(JobLaunchFault::Assignment);
    #[cfg(not(any(test, feature = "compat-adoption")))]
    let assignment_fault = false;
    let assigned = if assignment_fault {
        // SAFETY: deterministic adoption seam; same failure branch as a Win32 refusal.
        unsafe {
            windows_sys::Win32::Foundation::SetLastError(5);
        }
        0
    } else if overrides.job_at_creation {
        if member == 0 {
            stop_unstarted(&process, &mut overrides.trace);
            return Err(io::Error::other("created root did not enter its creation Job"));
        }
        1
    } else {
        // SAFETY: legacy restricted launches remain suspended with valid, non-inherited handles.
        unsafe { AssignProcessToJobObject(job.as_raw(), process.as_raw()) }
    };
    if assigned == 0 {
        let error = last_os_error_with_context("assign restricted process to job");
        stop_unstarted(&process, &mut overrides.trace);
        return Err(error);
    }
    if let Some((trace, origin)) = overrides.trace.as_mut() {
        trace.assigned = origin.elapsed();
    }
    // SAFETY: `thread` is the suspended primary thread returned above.
    #[cfg(any(test, feature = "compat-adoption"))]
    let resume_fault = overrides.fault == Some(JobLaunchFault::Resume);
    #[cfg(not(any(test, feature = "compat-adoption")))]
    let resume_fault = false;
    let resumed = if resume_fault {
        // SAFETY: deterministic seam before the first instruction can execute.
        unsafe {
            windows_sys::Win32::Foundation::SetLastError(5);
        }
        u32::MAX
    } else {
        unsafe { ResumeThread(thread.as_raw()) }
    };
    if resumed == u32::MAX {
        let error = last_os_error_with_context("resume restricted process");
        stop_unstarted(&process, &mut overrides.trace);
        return Err(error);
    }
    if let Some((trace, origin)) = overrides.trace.as_mut() {
        trace.resumed = origin.elapsed();
    }
    drop(thread);
    Ok(process)
}

fn stop_unstarted(process: &OwnedHandle, trace: &mut Option<(&mut JobLaunchTiming, Instant)>) {
    // SAFETY: exact suspended process owned by this launch. No PID-based takeover.
    let stopped = unsafe {
        TerminateProcess(process.as_raw(), 1) != 0
            && WaitForSingleObject(process.as_raw(), 5_000) == WAIT_OBJECT_0
    };
    if let Some((trace, _)) = trace.as_mut() {
        trace.failure_stopped = stopped;
    }
}
