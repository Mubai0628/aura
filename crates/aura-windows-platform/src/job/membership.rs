//! Queries are bound to an owned Job and a creation identity, never a bare PID.
use super::*;
use crate::process::{ProcessIdentity, snapshot_process_handle};
use windows_sys::Win32::{
    Foundation::{
        DUPLICATE_SAME_ACCESS, DuplicateHandle, ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, HANDLE,
    },
    System::{
        JobObjects::{
            IsProcessInJob, JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList,
        },
        Threading::{GetCurrentProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
    },
};

impl RootJob {
    pub(crate) fn duplicate_for_launch(&self) -> io::Result<OwnedHandle> {
        let mut duplicate: HANDLE = ptr::null_mut();
        // SAFETY: duplicate this live Job into this same process, without inheritance.
        if unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                self.handle.as_raw(),
                GetCurrentProcess(),
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        OwnedHandle::from_raw(duplicate)
    }

    pub fn contains_identity(&self, expected: &ProcessIdentity) -> io::Result<bool> {
        // SAFETY: query-only, non-inheritable process handle, immediately owned.
        let handle = OwnedHandle::from_raw(unsafe {
            OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, expected.process_id)
        })?;
        if snapshot_process_handle(handle.as_raw(), expected.process_id)? != *expected {
            return Ok(false);
        }
        let mut present = 0;
        // SAFETY: both exact live handles and initialized BOOL output.
        if unsafe { IsProcessInJob(handle.as_raw(), self.handle.as_raw(), &mut present) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(present != 0)
    }

    /// Bounded, potentially changing observation; a query failure is not an empty Job.
    pub fn process_identities(&self) -> io::Result<Vec<ProcessIdentity>> {
        let mut capacity = 16usize;
        loop {
            // Vec<usize> gives the flexible Win32 record its required alignment.
            let header_bytes = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList);
            let bytes = header_bytes + capacity * size_of::<usize>();
            let mut storage = vec![0usize; bytes.div_ceil(size_of::<usize>())];
            // SAFETY: aligned writable buffer, exact allocated byte length, owned Job.
            let success = unsafe {
                QueryInformationJobObject(
                    self.handle.as_raw(),
                    JobObjectBasicProcessIdList,
                    storage.as_mut_ptr().cast(),
                    bytes as u32,
                    ptr::null_mut(),
                )
            };
            if success == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_MORE_DATA as i32) && capacity < 4096 {
                    capacity *= 2;
                    continue;
                }
                return Err(error);
            }
            // SAFETY: buffer contains the requested record and at least one flexible slot.
            let record = unsafe { &*storage.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            let count = record.NumberOfProcessIdsInList as usize;
            if count > capacity {
                return Err(io::Error::other("Job process-list length exceeds buffer"));
            }
            // SAFETY: exact reported count was bounded above against this buffer.
            let ids = unsafe {
                std::slice::from_raw_parts(
                    storage.as_ptr().cast::<u8>().add(header_bytes).cast::<usize>(),
                    count,
                )
            };
            let mut identities = Vec::with_capacity(count);
            for pid in ids {
                let pid = u32::try_from(*pid).map_err(|_| io::Error::other("Job PID overflow"))?;
                let identity = match crate::snapshot_process(pid) {
                    Ok(identity) => identity,
                    Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {
                        continue;
                    }
                    Err(error) => {
                        return Err(io::Error::new(
                            error.kind(),
                            format!("Job member {pid}: {error}"),
                        ));
                    }
                };
                match self.contains_identity(&identity) {
                    Ok(true) => identities.push(identity),
                    Ok(false) => {}
                    Err(error) if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) => {}
                    Err(error) => {
                        return Err(io::Error::new(
                            error.kind(),
                            format!("Job membership {pid}: {error}"),
                        ));
                    }
                }
            }
            return Ok(identities);
        }
    }
}
