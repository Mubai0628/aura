//! Native boundary for the approved fixed resource maintainer. No arbitrary
//! object path, supplied session token, process launch or networking interface.
mod namespace;
mod service;
mod session;
pub use namespace::{MaintenanceNamespace, namespace_plan, remove_recorded_namespace};
pub use service::{
    MaintenanceEvent, migration_service_quiesced, process_query_confirms_absence,
    require_service_stopped, run_maintenance_service, service_running, stopping,
};
pub use session::{MaintenanceSession, boot_identity, current_logon_identity, owner_sessions};
pub const SERVICE_NAME: &str = "AuraExecutionResourceMaintainer";

use std::{
    io,
    path::{Path, PathBuf},
};
pub fn program_directory() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::Shell::{
        CSIDL_PROGRAM_FILES, SHGFP_TYPE_CURRENT, SHGetFolderPathW,
    };
    let mut path = [0u16; 260];
    // SAFETY: fixed system folder, no environment or client path.
    let status = unsafe {
        SHGetFolderPathW(
            std::ptr::null_mut(),
            CSIDL_PROGRAM_FILES as i32,
            std::ptr::null_mut(),
            SHGFP_TYPE_CURRENT as u32,
            path.as_mut_ptr(),
        )
    };
    if status < 0 {
        return Err(io::Error::other("maintenance.program_files_unavailable"));
    }
    let len = path
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| io::Error::other("maintenance.path_length"))?;
    Ok(PathBuf::from(std::ffi::OsString::from_wide(&path[..len])).join("Aura/ExecutionMaintainer"))
}
pub fn data_directory() -> io::Result<PathBuf> {
    Ok(crate::common_application_data_directory()?.join("Aura/ExecutionMaintainer"))
}
/// No normal owner write exception: SYSTEM's inputs require a BA/SYSTEM owner
/// and only BA/SYSTEM write access. Keep the checked handles open in callers.
pub fn verify_protected_path(path: &Path) -> io::Result<()> {
    for parent in path.ancestors() {
        if parent.is_absolute() && crate::inspect_existing_path(parent, false)?.is_reparse_point {
            return Err(io::Error::other("maintenance.reparse_path"));
        }
    }
    crate::verify_execution_maintenance_acl(path, true)?;
    for ancestor in path.ancestors().skip(1) {
        if ancestor.is_absolute() {
            crate::verify_execution_maintenance_acl(ancestor, false)?;
        }
    }
    Ok(())
}
pub fn require_system() -> io::Result<()> {
    let (sid, restricted) = crate::current_user_token_identity()?;
    if sid != "S-1-5-18" || restricted {
        return Err(io::Error::other("maintenance.system_required"));
    }
    Ok(())
}
