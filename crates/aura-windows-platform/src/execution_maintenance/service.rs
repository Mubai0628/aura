//! The SCM callback queues only refresh/stop; it never trusts event payloads.
//! Resource work runs serially outside the SCM control handler.
use std::{
    io,
    mem::size_of,
    ptr,
    sync::{
        OnceLock,
        mpsc::{self, Receiver, SyncSender},
    },
};
use windows_sys::Win32::{
    Foundation::{ERROR_CALL_NOT_IMPLEMENTED, ERROR_INVALID_PARAMETER, ERROR_SUCCESS},
    System::Services::*,
};
#[derive(Clone, Copy, Debug)]
pub enum MaintenanceEvent {
    Refresh,
    Stop,
}
type Worker = fn(Receiver<MaintenanceEvent>) -> io::Result<()>;
/// Fixed read-only SCM query, never a custom control or a resource mutation.
pub fn service_running() -> io::Result<bool> {
    struct ServiceHandle(SC_HANDLE);
    impl Drop for ServiceHandle {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns a successful SCM handle.
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let manager = ServiceHandle(manager);
    let name = super::SERVICE_NAME.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let service = unsafe { OpenServiceW(manager.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if service.is_null() {
        return Err(io::Error::last_os_error());
    }
    let service = ServiceHandle(service);
    let mut status = SERVICE_STATUS::default();
    if unsafe { QueryServiceStatus(service.0, &mut status) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(status.dwCurrentState == SERVICE_RUNNING)
}

/// SERVICE_RUNNING is not the inverse of stopped: START_PENDING and
/// STOP_PENDING also fail this check. The recorded service PID must be
/// confirmed gone. Query failures are not absence.
pub fn process_query_confirms_absence(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        || error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32)
}

pub fn migration_service_quiesced(
    state: u32,
    process_id: u32,
    process: io::Result<bool>,
) -> io::Result<()> {
    if state != SERVICE_STOPPED {
        return Err(io::Error::other("maintenance.migration_service_running"));
    }
    if process_id == 0 {
        return Ok(());
    }
    match process {
        Ok(true) => Err(io::Error::other("maintenance.migration_service_process")),
        Ok(false) => Ok(()),
        Err(error) => Err(io::Error::new(
            error.kind(),
            format!("maintenance.migration_service_process_query: {error}"),
        )),
    }
}

pub fn require_service_stopped() -> io::Result<()> {
    let (state, process_id) = service_process_status()?;
    let process = query_recorded_service_process(process_id);
    migration_service_quiesced(state, process_id, process)
}

fn query_recorded_service_process(process_id: u32) -> io::Result<bool> {
    if process_id == 0 {
        return Ok(false);
    }
    match crate::snapshot_process(process_id) {
        Ok(_) => Ok(true),
        Err(error) if process_query_confirms_absence(&error) => Ok(false),
        Err(error) => Err(error),
    }
}

fn service_process_status() -> io::Result<(u32, u32)> {
    struct ServiceHandle(SC_HANDLE);
    impl Drop for ServiceHandle {
        fn drop(&mut self) {
            // SAFETY: this wrapper owns a successful SCM handle.
            unsafe {
                CloseServiceHandle(self.0);
            }
        }
    }
    let manager = unsafe { OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT) };
    if manager.is_null() {
        return Err(io::Error::last_os_error());
    }
    let manager = ServiceHandle(manager);
    let name = super::SERVICE_NAME.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let service = unsafe { OpenServiceW(manager.0, name.as_ptr(), SERVICE_QUERY_STATUS) };
    if service.is_null() {
        return Err(io::Error::last_os_error());
    }
    let service = ServiceHandle(service);
    let mut status = SERVICE_STATUS_PROCESS::default();
    let mut needed = 0u32;
    // SAFETY: SC_STATUS_PROCESS_INFO writes SERVICE_STATUS_PROCESS into status.
    if unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            ptr::from_mut(&mut status).cast(),
            u32::try_from(size_of::<SERVICE_STATUS_PROCESS>())
                .map_err(|_| io::Error::other("maintenance.service_status_size"))?,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok((status.dwCurrentState, status.dwProcessId))
}
static WORKER: OnceLock<Worker> = OnceLock::new();
static EVENTS: OnceLock<SyncSender<MaintenanceEvent>> = OnceLock::new();

pub fn run_maintenance_service(worker: Worker) -> io::Result<()> {
    super::require_system()?;
    WORKER.set(worker).map_err(|_| io::Error::other("maintenance.already_started"))?;
    let mut name = super::SERVICE_NAME.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let table = [
        SERVICE_TABLE_ENTRYW {
            lpServiceName: name.as_mut_ptr(),
            lpServiceProc: Some(service_main),
        },
        SERVICE_TABLE_ENTRYW::default(),
    ];
    // SAFETY: fixed single-service table and terminal zero entry live until SCM
    // dispatch returns. This is not a callable CLI or custom control interface.
    if unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
unsafe extern "system" fn handle(
    control: u32,
    _event: u32,
    _data: *mut core::ffi::c_void,
    _context: *mut core::ffi::c_void,
) -> u32 {
    match control {
        SERVICE_CONTROL_INTERROGATE => ERROR_SUCCESS,
        SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN => {
            STOP.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(tx) = EVENTS.get() {
                let _ = tx.try_send(MaintenanceEvent::Stop);
            }
            ERROR_SUCCESS
        }
        SERVICE_CONTROL_SESSIONCHANGE => {
            if let Some(tx) = EVENTS.get() {
                let _ = tx.try_send(MaintenanceEvent::Refresh);
            }
            ERROR_SUCCESS
        }
        _ => ERROR_CALL_NOT_IMPLEMENTED,
    }
}
static STOP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub fn stopping() -> bool {
    STOP.load(std::sync::atomic::Ordering::SeqCst)
}
unsafe extern "system" fn service_main(_count: u32, _args: *mut *mut u16) {
    let name = super::SERVICE_NAME.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    // SAFETY: fixed name, stateless callback; no client context.
    let status = unsafe { RegisterServiceCtrlHandlerExW(name.as_ptr(), Some(handle), ptr::null()) };
    if status.is_null() {
        return;
    }
    let (tx, rx) = mpsc::sync_channel(8);
    let _ = EVENTS.set(tx);
    let running = SERVICE_STATUS {
        dwServiceType: SERVICE_WIN32_OWN_PROCESS,
        dwCurrentState: SERVICE_RUNNING,
        dwControlsAccepted: SERVICE_ACCEPT_STOP
            | SERVICE_ACCEPT_SHUTDOWN
            | SERVICE_ACCEPT_SESSIONCHANGE,
        dwWin32ExitCode: 0,
        dwServiceSpecificExitCode: 0,
        dwCheckPoint: 0,
        dwWaitHint: 0,
    };
    unsafe {
        SetServiceStatus(status, &running);
    }
    // Catch panics at the FFI boundary. The worker itself publishes failures;
    // SCM running means service health, never backend resource readiness.
    let result = std::panic::catch_unwind(|| {
        WORKER.get().ok_or_else(|| io::Error::other("maintenance.worker_missing"))?(rx)
    });
    let code = if matches!(result, Ok(Ok(()))) { 0 } else { 1066 };
    let stopped = SERVICE_STATUS {
        dwCurrentState: SERVICE_STOPPED,
        dwControlsAccepted: 0,
        dwWin32ExitCode: code,
        ..running
    };
    unsafe {
        SetServiceStatus(status, &stopped);
    }
}

#[cfg(test)]
mod tests {
    use super::{migration_service_quiesced, process_query_confirms_absence};
    use std::io;
    use windows_sys::Win32::{
        Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER},
        System::Services::{
            SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STOP_PENDING, SERVICE_STOPPED,
        },
    };

    #[test]
    fn stop_pending_and_lingering_pid_are_not_stopped() {
        assert!(migration_service_quiesced(SERVICE_STOP_PENDING, 0, Ok(false)).is_err());
        assert!(migration_service_quiesced(SERVICE_START_PENDING, 0, Ok(false)).is_err());
        assert!(migration_service_quiesced(SERVICE_RUNNING, 0, Ok(false)).is_err());
        assert!(migration_service_quiesced(SERVICE_STOPPED, 0, Ok(false)).is_ok());
        assert!(migration_service_quiesced(SERVICE_STOPPED, 0, Ok(true)).is_ok());
        assert!(migration_service_quiesced(SERVICE_STOPPED, 4228, Ok(true)).is_err());
        assert!(migration_service_quiesced(SERVICE_STOPPED, 4228, Ok(false)).is_ok());
    }

    #[test]
    fn query_refusal_is_not_process_absence() {
        let denied = io::Error::from_raw_os_error(ERROR_ACCESS_DENIED as i32);
        assert!(!process_query_confirms_absence(&denied));
        let error =
            migration_service_quiesced(SERVICE_STOPPED, 4228, Err(denied)).expect_err("query");
        assert!(
            error.to_string().contains("maintenance.migration_service_process_query"),
            "{error}"
        );
        let identity = io::Error::other("process identity changed");
        assert!(!process_query_confirms_absence(&identity));
        assert!(migration_service_quiesced(SERVICE_STOPPED, 4228, Err(identity)).is_err());
        let missing = io::Error::from_raw_os_error(ERROR_INVALID_PARAMETER as i32);
        assert!(process_query_confirms_absence(&missing));
    }
}
