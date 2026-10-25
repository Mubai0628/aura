//! Microsoft Windows system-component catalog trust for the OS CMD image.
//! This is not Aura release trust and does not waive file identity for Core,
//! Runner, or pwsh.
use std::{
    ffi::CStr,
    io,
    mem::size_of,
    os::windows::io::AsRawHandle,
    path::Path,
    ptr::{null, null_mut},
};

use windows_sys::{
    Win32::{
        Foundation::{HANDLE, HWND, INVALID_HANDLE_VALUE},
        Security::{
            Cryptography::{
                CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL, CERT_CHAIN_DISABLE_AIA, CERT_CHAIN_PARA,
                CERT_CHAIN_POLICY_MICROSOFT_ROOT, CERT_CHAIN_POLICY_PARA, CERT_CHAIN_POLICY_STATUS,
                CERT_CONTEXT, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG, CTL_USAGE,
                Catalog::{
                    CATALOG_INFO, CryptCATAdminAcquireContext, CryptCATAdminAcquireContext2,
                    CryptCATAdminCalcHashFromFileHandle, CryptCATAdminCalcHashFromFileHandle2,
                    CryptCATAdminEnumCatalogFromHash, CryptCATAdminReleaseCatalogContext,
                    CryptCATAdminReleaseContext, CryptCATCatalogInfoFromContext,
                },
                CertFreeCertificateChain, CertGetCertificateChain, CertGetEnhancedKeyUsage,
                CertVerifyCertificateChainPolicy, szOID_NT5_CRYPTO,
            },
            WinTrust::{
                DRIVER_ACTION_VERIFY, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_CATALOG_INFO,
                WINTRUST_DATA, WINTRUST_DATA_0, WTD_CACHE_ONLY_URL_RETRIEVAL, WTD_CHOICE_CATALOG,
                WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE, WTD_STATEACTION_VERIFY, WTD_UI_NONE,
                WTHelperGetProvCertFromChain, WTHelperGetProvSignerFromChain,
                WTHelperProvDataFromStateData, WinVerifyTrustEx,
            },
        },
        Storage::FileSystem::{FILE_BEGIN, SetFilePointerEx},
    },
    core::PCSTR,
};

/// Windows System Component Verification EKU (OID, not a display name).
pub const WINDOWS_SYSTEM_COMPONENT_EKU: &str = "1.3.6.1.4.1.311.10.3.6";

pub fn verify_microsoft_catalog_executable(file: &std::fs::File, path: &Path) -> io::Result<()> {
    let handle = file.as_raw_handle() as HANDLE;
    let display = wintrust_path(path);
    if verify_catalog(handle, &display, true).is_ok() {
        return Ok(());
    }
    verify_catalog(handle, &display, false)
}

/// Origin gate used by tests: Microsoft root plus the Windows system-component
/// EKU. Catalog membership alone is not enough.
pub fn windows_system_component_policy(eku_oids: &[&str], microsoft_root: bool) -> io::Result<()> {
    if microsoft_root && eku_oids.contains(&WINDOWS_SYSTEM_COMPONENT_EKU) {
        Ok(())
    } else {
        Err(io::Error::other("runner.untrusted_cmd_image"))
    }
}

fn verify_catalog(handle: HANDLE, path: &str, sha256: bool) -> io::Result<()> {
    rewind_handle(handle)?;
    let mut admin = 0isize;
    let acquired = if sha256 {
        // SAFETY: optional algorithm is a static SHA256 string; no policy blob.
        unsafe {
            CryptCATAdminAcquireContext2(
                &mut admin,
                &DRIVER_ACTION_VERIFY,
                windows_sys::core::w!("SHA256"),
                null(),
                0,
            )
        }
    } else {
        // SAFETY: DRIVER_ACTION_VERIFY selects the OS component catalog set.
        unsafe { CryptCATAdminAcquireContext(&mut admin, &DRIVER_ACTION_VERIFY, 0) }
    };
    if acquired == 0 || admin == 0 {
        return Err(io::Error::last_os_error());
    }
    struct Admin(isize);
    impl Drop for Admin {
        fn drop(&mut self) {
            unsafe { CryptCATAdminReleaseContext(self.0, 0) };
        }
    }
    let admin = Admin(admin);

    let mut hash_len = 0u32;
    let hashed = if sha256 {
        unsafe {
            CryptCATAdminCalcHashFromFileHandle2(admin.0, handle, &mut hash_len, null_mut(), 0)
        }
    } else {
        unsafe { CryptCATAdminCalcHashFromFileHandle(handle, &mut hash_len, null_mut(), 0) }
    };
    if hashed == 0 || hash_len == 0 {
        return Err(io::Error::last_os_error());
    }
    rewind_handle(handle)?;
    let mut hash = vec![0u8; hash_len as usize];
    let hashed = if sha256 {
        unsafe {
            CryptCATAdminCalcHashFromFileHandle2(
                admin.0,
                handle,
                &mut hash_len,
                hash.as_mut_ptr(),
                0,
            )
        }
    } else {
        unsafe { CryptCATAdminCalcHashFromFileHandle(handle, &mut hash_len, hash.as_mut_ptr(), 0) }
    };
    if hashed == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut prev = 0isize;
    // SAFETY: hash buffer matches the length returned by the sizing call.
    let catalog =
        unsafe { CryptCATAdminEnumCatalogFromHash(admin.0, hash.as_ptr(), hash_len, 0, &mut prev) };
    if catalog == 0 {
        return Err(io::Error::other("runner.untrusted_cmd_image"));
    }
    struct Catalog {
        admin: isize,
        info: isize,
    }
    impl Drop for Catalog {
        fn drop(&mut self) {
            unsafe { CryptCATAdminReleaseCatalogContext(self.admin, self.info, 0) };
        }
    }
    let catalog = Catalog { admin: admin.0, info: catalog };

    let mut info =
        CATALOG_INFO { cbStruct: size_of::<CATALOG_INFO>() as u32, wszCatalogFile: [0; 260] };
    if unsafe { CryptCATCatalogInfoFromContext(catalog.info, &mut info, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let tag = hash.iter().map(|byte| format!("{byte:02X}")).collect::<String>();
    let tag_wide = wide_null(&tag);
    let path_wide = wide_null(path);
    let mut catalog_info = WINTRUST_CATALOG_INFO {
        cbStruct: size_of::<WINTRUST_CATALOG_INFO>() as u32,
        dwCatalogVersion: 0,
        pcwszCatalogFilePath: info.wszCatalogFile.as_ptr(),
        pcwszMemberTag: tag_wide.as_ptr(),
        pcwszMemberFilePath: path_wide.as_ptr(),
        hMemberFile: handle,
        pbCalculatedFileHash: hash.as_mut_ptr(),
        cbCalculatedFileHash: hash_len,
        pcCatalogContext: null_mut(),
        hCatAdmin: admin.0,
    };
    let mut data = WINTRUST_DATA {
        cbStruct: size_of::<WINTRUST_DATA>() as u32,
        pPolicyCallbackData: null_mut(),
        pSIPClientData: null_mut(),
        dwUIChoice: WTD_UI_NONE,
        fdwRevocationChecks: WTD_REVOKE_NONE,
        dwUnionChoice: WTD_CHOICE_CATALOG,
        Anonymous: WINTRUST_DATA_0 { pCatalog: &mut catalog_info },
        dwStateAction: WTD_STATEACTION_VERIFY,
        hWVTStateData: null_mut(),
        pwszURLReference: null_mut(),
        dwProvFlags: WTD_CACHE_ONLY_URL_RETRIEVAL,
        dwUIContext: 0,
        pSignatureSettings: null_mut(),
    };
    let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;
    // SAFETY: catalog/member pointers outlive VERIFY, origin extraction, and CLOSE.
    let status = unsafe { WinVerifyTrustEx(INVALID_HANDLE_VALUE as HWND, &mut action, &mut data) };
    let origin =
        if status == 0 { verify_windows_system_component(data.hWVTStateData) } else { Err(()) };
    data.dwStateAction = WTD_STATEACTION_CLOSE;
    let _ = unsafe { WinVerifyTrustEx(INVALID_HANDLE_VALUE as HWND, &mut action, &mut data) };
    drop(catalog);
    drop(admin);
    if status == 0 {
        origin.map_err(|_| io::Error::other("runner.untrusted_cmd_image"))
    } else {
        Err(io::Error::other(format!("runner.untrusted_cmd_image:{status:#x}")))
    }
}

fn verify_windows_system_component(state: HANDLE) -> Result<(), ()> {
    if state.is_null() {
        return Err(());
    }
    // SAFETY: hWVTStateData is live until CLOSE; helpers borrow it.
    let provider = unsafe { WTHelperProvDataFromStateData(state) };
    if provider.is_null() {
        return Err(());
    }
    let signer = unsafe { WTHelperGetProvSignerFromChain(provider, 0, 0, 0) };
    if signer.is_null() {
        return Err(());
    }
    let cert_info = unsafe { WTHelperGetProvCertFromChain(signer, 0) };
    if cert_info.is_null() {
        return Err(());
    }
    let cert = unsafe { (*cert_info).pCert };
    if cert.is_null() {
        return Err(());
    }
    let oids = enhanced_key_usage(cert).map_err(|_| ())?;
    let oid_refs: Vec<&str> = oids.iter().map(String::as_str).collect();
    let chain = unsafe { (*signer).pChainContext };
    let microsoft_root = if chain.is_null() {
        microsoft_root_from_cert(cert)?
    } else {
        microsoft_root_policy(chain)
    };
    windows_system_component_policy(&oid_refs, microsoft_root).map_err(|_| ())
}

fn enhanced_key_usage(cert: *const CERT_CONTEXT) -> io::Result<Vec<String>> {
    let mut oids = read_eku(cert, CERT_FIND_EXT_ONLY_ENHKEY_USAGE_FLAG);
    if oids.is_empty() {
        oids = read_eku(cert, 0);
    }
    if oids.is_empty() {
        return Err(io::Error::other("runner.untrusted_cmd_image"));
    }
    Ok(oids)
}

fn read_eku(cert: *const CERT_CONTEXT, flags: u32) -> Vec<String> {
    let mut usage_size = 0u32;
    // SAFETY: sizing call; output pointer is null.
    if unsafe { CertGetEnhancedKeyUsage(cert, flags, null_mut(), &mut usage_size) } == 0
        || usage_size < size_of::<CTL_USAGE>() as u32
    {
        return Vec::new();
    }
    let mut buffer = vec![0usize; (usage_size as usize).div_ceil(size_of::<usize>())];
    let mut actual = usage_size;
    // SAFETY: aligned buffer is at least the reported CTL_USAGE size.
    if unsafe { CertGetEnhancedKeyUsage(cert, flags, buffer.as_mut_ptr().cast(), &mut actual) } == 0
    {
        return Vec::new();
    }
    // SAFETY: CryptoAPI filled a CTL_USAGE.
    let usage = unsafe { &*buffer.as_ptr().cast::<CTL_USAGE>() };
    if usage.cUsageIdentifier == 0 || usage.rgpszUsageIdentifier.is_null() {
        return Vec::new();
    }
    let mut oids = Vec::new();
    for index in 0..usage.cUsageIdentifier as usize {
        // SAFETY: cUsageIdentifier entries were returned by CryptoAPI.
        let ptr = unsafe { *usage.rgpszUsageIdentifier.add(index) };
        if let Some(oid) = oid_text(ptr) {
            oids.push(oid);
        }
    }
    oids
}

fn oid_text(oid: windows_sys::core::PSTR) -> Option<String> {
    if oid.is_null() {
        return None;
    }
    // SAFETY: CryptoAPI EKU strings are NUL-terminated.
    unsafe { CStr::from_ptr(oid.cast()) }.to_str().ok().map(str::to_owned)
}

fn microsoft_root_from_cert(cert: *const CERT_CONTEXT) -> Result<bool, ()> {
    let mut chain = null_mut();
    let chain_parameters =
        CERT_CHAIN_PARA { cbSize: size_of::<CERT_CHAIN_PARA>() as u32, ..Default::default() };
    let flags = CERT_CHAIN_CACHE_ONLY_URL_RETRIEVAL | CERT_CHAIN_DISABLE_AIA;
    // SAFETY: default engine; cert remains live for the call.
    let ok = unsafe {
        CertGetCertificateChain(
            null_mut(),
            cert,
            null(),
            null_mut(),
            &raw const chain_parameters,
            flags,
            null_mut(),
            &mut chain,
        )
    };
    if ok == 0 || chain.is_null() {
        return Err(());
    }
    let trusted = microsoft_root_policy(chain);
    unsafe { CertFreeCertificateChain(chain) };
    Ok(trusted)
}

fn microsoft_root_policy(
    chain: *const windows_sys::Win32::Security::Cryptography::CERT_CHAIN_CONTEXT,
) -> bool {
    let policy = CERT_CHAIN_POLICY_PARA {
        cbSize: size_of::<CERT_CHAIN_POLICY_PARA>() as u32,
        ..Default::default()
    };
    let mut status = CERT_CHAIN_POLICY_STATUS {
        cbSize: size_of::<CERT_CHAIN_POLICY_STATUS>() as u32,
        ..Default::default()
    };
    // SAFETY: chain and policy buffers are live for the documented Microsoft-root policy.
    let ok = unsafe {
        CertVerifyCertificateChainPolicy(
            CERT_CHAIN_POLICY_MICROSOFT_ROOT,
            chain,
            &raw const policy,
            &mut status,
        )
    };
    ok != 0 && status.dwError == 0
}

fn rewind_handle(handle: HANDLE) -> io::Result<()> {
    // SAFETY: the caller retains the file; this only resets the file pointer.
    if unsafe { SetFilePointerEx(handle, 0, null_mut(), FILE_BEGIN) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wintrust_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix(r"\\?\").unwrap_or(text.as_ref()).replace('/', "\\")
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

const _: PCSTR = szOID_NT5_CRYPTO;

#[cfg(test)]
mod tests {
    use super::{
        WINDOWS_SYSTEM_COMPONENT_EKU, verify_microsoft_catalog_executable,
        windows_system_component_policy,
    };
    use std::fs::File;

    #[test]
    fn windows_cmd_has_a_microsoft_catalog_signature() {
        let cmd = crate::windows_directory().expect("Windows").join("System32/cmd.exe");
        let file = File::open(&cmd).expect("open cmd");
        verify_microsoft_catalog_executable(&file, &cmd).expect("catalog");
    }

    #[test]
    fn unsigned_bytes_are_not_a_trusted_system_cmd() {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("not-cmd.exe");
        std::fs::write(&path, b"MZ-not-a-signed-image").expect("write");
        let file = File::open(&path).expect("open");
        let error = verify_microsoft_catalog_executable(&file, &path).expect_err("unsigned");
        assert!(error.to_string().contains("runner.untrusted_cmd_image"), "{error}");
    }

    #[test]
    fn authenticode_code_signing_alone_is_not_a_windows_system_component() {
        let code_signing = "1.3.6.1.5.5.7.3.3";
        let whql = "1.3.6.1.4.1.311.10.3.5";
        assert!(windows_system_component_policy(&[code_signing], true).is_err());
        assert!(windows_system_component_policy(&[whql], true).is_err());
        assert!(windows_system_component_policy(&[WINDOWS_SYSTEM_COMPONENT_EKU], false).is_err());
        windows_system_component_policy(&[WINDOWS_SYSTEM_COMPONENT_EKU], true).expect("component");
        windows_system_component_policy(&[code_signing, WINDOWS_SYSTEM_COMPONENT_EKU], true)
            .expect("component with extra eku");
    }
}
