//! Stable storage for handle, security, Job and pseudoconsole startup attributes.
use std::{ffi::c_void, io, mem::size_of, ptr};
use windows_sys::Win32::{
    Foundation::{ERROR_INSUFFICIENT_BUFFER, GetLastError, HANDLE},
    Security::{PSID, SECURITY_CAPABILITIES},
    System::{
        Console::HPCON,
        Threading::{
            DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_JOB_LIST,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            UpdateProcThreadAttribute,
        },
    },
};

pub(crate) struct ProcessAttributeList {
    storage: Vec<usize>,
    handles: Box<[HANDLE]>,
    jobs: Box<[HANDLE]>,
    capabilities: Option<Box<SECURITY_CAPABILITIES>>,
}
impl ProcessAttributeList {
    fn allocate(count: u32) -> io::Result<Self> {
        let mut required = 0;
        // SAFETY: documented sizing call, followed by aligned owned storage.
        let sized =
            unsafe { InitializeProcThreadAttributeList(ptr::null_mut(), count, 0, &mut required) };
        let error = unsafe { GetLastError() };
        if sized != 0 || error != ERROR_INSUFFICIENT_BUFFER || required == 0 {
            return Err(io::Error::from_raw_os_error(error as i32));
        }
        let mut storage = vec![0usize; required.div_ceil(size_of::<usize>())];
        if unsafe {
            InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(), count, 0, &mut required)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self { storage, handles: Box::new([]), jobs: Box::new([]), capabilities: None })
    }
    pub(crate) fn new(sid: Option<PSID>, handles: Vec<HANDLE>) -> io::Result<Self> {
        Self::handles_with_job(sid, handles, None)
    }
    pub(crate) fn with_job(
        sid: Option<PSID>,
        handles: Vec<HANDLE>,
        job: HANDLE,
    ) -> io::Result<Self> {
        Self::handles_with_job(sid, handles, Some(job))
    }
    fn handles_with_job(
        sid: Option<PSID>,
        handles: Vec<HANDLE>,
        job: Option<HANDLE>,
    ) -> io::Result<Self> {
        if handles.is_empty() || handles.iter().any(|handle| handle.is_null()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "process handle inheritance list is invalid",
            ));
        }
        let mut list = Self::allocate(1 + u32::from(sid.is_some()) + u32::from(job.is_some()))?;
        list.handles = handles.into_boxed_slice();
        // SAFETY: boxed handle storage remains stable until the list is destroyed.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_raw(),
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                list.handles.as_mut_ptr().cast(),
                list.handles.len() * size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        list.capabilities = sid.map(|sid| {
            Box::new(SECURITY_CAPABILITIES {
                AppContainerSid: sid,
                Capabilities: ptr::null_mut(),
                CapabilityCount: 0,
                Reserved: 0,
            })
        });
        if let Some(capabilities) = list.capabilities.as_mut() {
            let value = capabilities.as_mut() as *mut SECURITY_CAPABILITIES;
            if unsafe {
                UpdateProcThreadAttribute(
                    list.as_raw(),
                    0,
                    PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
                    value.cast(),
                    size_of::<SECURITY_CAPABILITIES>(),
                    ptr::null_mut(),
                    ptr::null(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        if let Some(job) = job {
            list.install_job(job)?;
        }
        Ok(list)
    }
    pub(crate) fn job_only(job: HANDLE) -> io::Result<Self> {
        let mut list = Self::allocate(1)?;
        list.install_job(job)?;
        Ok(list)
    }
    pub(crate) fn with_pseudoconsole(console: HPCON, job: HANDLE) -> io::Result<Self> {
        let mut list = Self::allocate(2)?;
        // SAFETY: ConPTY documents the HPCON value itself, not a pointer to it.
        // The caller owns the console throughout CreateProcess.
        if unsafe {
            UpdateProcThreadAttribute(
                list.as_raw(),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                console as *mut c_void,
                size_of::<HPCON>(),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        list.install_job(job)?;
        Ok(list)
    }
    fn install_job(&mut self, job: HANDLE) -> io::Result<()> {
        if job.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "creation Job handle is invalid",
            ));
        }
        self.jobs = Box::new([job]);
        // SAFETY: JOB_LIST takes a HANDLE array. Its boxed address stays stable for the
        // entire attribute-list lifetime; the caller keeps this non-inheritable Job open
        // through CreateProcess. Windows assigns it before the root's first instruction.
        if unsafe {
            UpdateProcThreadAttribute(
                self.as_raw(),
                0,
                PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                self.jobs.as_mut_ptr().cast(),
                self.jobs.len() * size_of::<HANDLE>(),
                ptr::null_mut(),
                ptr::null(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub(crate) fn as_raw(&mut self) -> *mut c_void {
        self.storage.as_mut_ptr().cast()
    }
}
impl Drop for ProcessAttributeList {
    fn drop(&mut self) {
        // SAFETY: only initialized lists are constructed; stable storage is still live.
        unsafe { DeleteProcThreadAttributeList(self.storage.as_mut_ptr().cast()) };
    }
}
