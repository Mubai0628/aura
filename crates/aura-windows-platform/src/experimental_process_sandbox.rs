//! Explicitly opt-in probe for the experimental Windows ProcessModel sandbox API.
//!
//! This module deliberately does not define or call the experimental process-creation ABI.
//! Microsoft does not currently publish a C header for the API and Aura does not vendor an
//! audited `SandboxSpec.fbs` binding. The probe therefore records host/export availability only.
//! It is not part of Aura's production sandbox selection or any release security guarantee.

use std::{iter, mem, ptr};

use windows_sys::Win32::{
    Foundation::{ERROR_DLL_NOT_FOUND, ERROR_MOD_NOT_FOUND, FreeLibrary, GetLastError, HMODULE},
    System::LibraryLoader::{GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW},
};

pub const EXPERIMENTAL_PROCESSMODEL_DOCUMENTATION_URL: &str =
    "https://learn.microsoft.com/windows/win32/secauthz/createprocessinsandbox";

const CREATE_PROCESS_EXPORT: &[u8] = b"Experimental_CreateProcessInSandbox\0";
const CREATE_PROCESS_AS_USER_EXPORT: &[u8] = b"Experimental_CreateProcessAsUserInSandbox\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExperimentalProcessModelProxyStatus {
    Passed,
    Failed,
    Unsupported,
    Inconclusive,
}

impl ExperimentalProcessModelProxyStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Unsupported => "unsupported",
            Self::Inconclusive => "inconclusive",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExperimentalProcessModelProxyProbe {
    pub status: ExperimentalProcessModelProxyStatus,
    pub reason_code: &'static str,
    pub summary: String,
    pub processmodel_dll_loaded: bool,
    pub create_process_export_available: bool,
    pub create_process_as_user_export_available: bool,
    pub loader_error_code: Option<u32>,
    pub aura_audited_flatbuffer_binding_available: bool,
    pub api_invoked: bool,
    pub bypass_tests_executed: bool,
    pub cleanup_required: bool,
    pub cleanup_verified: bool,
    pub release_security_guarantee: bool,
}

/// Probes the experimental API without invoking its unpublished ABI.
///
/// `processmodel.dll` is resolved exclusively from System32, as required by Microsoft's API
/// documentation. An available export is reported as inconclusive until Aura has an audited
/// public FlatBuffer contract and can execute the bypass and compatibility matrix.
#[must_use]
pub fn probe_experimental_processmodel_proxy() -> ExperimentalProcessModelProxyProbe {
    let module = match LoadedSystemModule::load_processmodel() {
        Ok(module) => module,
        Err(error_code) => {
            let (status, reason_code, summary) = if matches!(
                error_code,
                ERROR_MOD_NOT_FOUND | ERROR_DLL_NOT_FOUND
            ) {
                (
                    ExperimentalProcessModelProxyStatus::Unsupported,
                    "processmodel_dll_unavailable",
                    format!(
                        "processmodel.dll is not available through the System32-only loader on this host (Win32 error {error_code})"
                    ),
                )
            } else {
                (
                    ExperimentalProcessModelProxyStatus::Inconclusive,
                    "processmodel_dll_probe_failed",
                    format!(
                        "processmodel.dll could not be probed through the System32-only loader (Win32 error {error_code}); API support could not be determined"
                    ),
                )
            };
            return ExperimentalProcessModelProxyProbe {
                status,
                reason_code,
                summary,
                processmodel_dll_loaded: false,
                create_process_export_available: false,
                create_process_as_user_export_available: false,
                loader_error_code: Some(error_code),
                aura_audited_flatbuffer_binding_available: false,
                api_invoked: false,
                bypass_tests_executed: false,
                cleanup_required: false,
                cleanup_verified: true,
                release_security_guarantee: false,
            };
        }
    };

    let create_process_export_available = module.has_export(CREATE_PROCESS_EXPORT);
    let create_process_as_user_export_available = module.has_export(CREATE_PROCESS_AS_USER_EXPORT);

    if let Err(error_code) = module.close() {
        return ExperimentalProcessModelProxyProbe {
            status: ExperimentalProcessModelProxyStatus::Failed,
            reason_code: "processmodel_library_cleanup_failed",
            summary: format!(
                "processmodel.dll was probed but FreeLibrary failed (Win32 error {error_code})"
            ),
            processmodel_dll_loaded: true,
            create_process_export_available,
            create_process_as_user_export_available,
            loader_error_code: Some(error_code),
            aura_audited_flatbuffer_binding_available: false,
            api_invoked: false,
            bypass_tests_executed: false,
            cleanup_required: true,
            cleanup_verified: false,
            release_security_guarantee: false,
        };
    }

    if !create_process_export_available {
        return ExperimentalProcessModelProxyProbe {
            status: ExperimentalProcessModelProxyStatus::Unsupported,
            reason_code: "experimental_create_process_export_unavailable",
            summary: String::from(
                "processmodel.dll is present, but Experimental_CreateProcessInSandbox is not exported on this host",
            ),
            processmodel_dll_loaded: true,
            create_process_export_available,
            create_process_as_user_export_available,
            loader_error_code: None,
            aura_audited_flatbuffer_binding_available: false,
            api_invoked: false,
            bypass_tests_executed: false,
            cleanup_required: true,
            cleanup_verified: true,
            release_security_guarantee: false,
        };
    }

    ExperimentalProcessModelProxyProbe {
        status: ExperimentalProcessModelProxyStatus::Inconclusive,
        reason_code: "audited_flatbuffer_binding_unavailable",
        summary: String::from(
            "Experimental_CreateProcessInSandbox is exported, but Aura has no audited public SandboxSpec.fbs binding; the API was not invoked and proxy enforcement remains unverified",
        ),
        processmodel_dll_loaded: true,
        create_process_export_available,
        create_process_as_user_export_available,
        loader_error_code: None,
        aura_audited_flatbuffer_binding_available: false,
        api_invoked: false,
        bypass_tests_executed: false,
        cleanup_required: true,
        cleanup_verified: true,
        release_security_guarantee: false,
    }
}

struct LoadedSystemModule {
    handle: HMODULE,
}

impl LoadedSystemModule {
    fn load_processmodel() -> Result<Self, u32> {
        let dll_name = "processmodel.dll".encode_utf16().chain(iter::once(0)).collect::<Vec<_>>();
        // SAFETY: `dll_name` is a live, NUL-terminated UTF-16 buffer; the NULL file handle is
        // required by LoadLibraryExW. LOAD_LIBRARY_SEARCH_SYSTEM32 prevents current-directory or
        // PATH-based DLL resolution.
        let handle = unsafe {
            LoadLibraryExW(dll_name.as_ptr(), ptr::null_mut(), LOAD_LIBRARY_SEARCH_SYSTEM32)
        };
        if handle.is_null() {
            // SAFETY: GetLastError has no preconditions and is called immediately after failure.
            Err(unsafe { GetLastError() })
        } else {
            Ok(Self { handle })
        }
    }

    fn has_export(&self, name: &[u8]) -> bool {
        // SAFETY: `self.handle` is a live module handle and both callers pass static,
        // NUL-terminated ASCII symbol names.
        unsafe { GetProcAddress(self.handle, name.as_ptr()) }.is_some()
    }

    fn close(mut self) -> Result<(), u32> {
        let handle = mem::replace(&mut self.handle, ptr::null_mut());
        // SAFETY: `handle` is owned by this wrapper and is released exactly once here.
        if unsafe { FreeLibrary(handle) } == 0 {
            // SAFETY: GetLastError has no preconditions and is called immediately after failure.
            Err(unsafe { GetLastError() })
        } else {
            Ok(())
        }
    }
}

impl Drop for LoadedSystemModule {
    fn drop(&mut self) {
        if !self.handle.is_null() {
            // SAFETY: This is the fallback release path for the still-owned module handle.
            let _ = unsafe { FreeLibrary(self.handle) };
            self.handle = ptr::null_mut();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_are_stable_for_the_standalone_report() {
        assert_eq!(ExperimentalProcessModelProxyStatus::Passed.as_str(), "passed");
        assert_eq!(ExperimentalProcessModelProxyStatus::Failed.as_str(), "failed");
        assert_eq!(ExperimentalProcessModelProxyStatus::Unsupported.as_str(), "unsupported");
        assert_eq!(ExperimentalProcessModelProxyStatus::Inconclusive.as_str(), "inconclusive");
    }
}
