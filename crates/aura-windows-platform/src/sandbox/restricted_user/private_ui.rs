//! Per-attempt private desktop, qualified with its actual parent station.
use super::*;
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, CreateDesktopW, GetProcessWindowStation, GetUserObjectInformationW, HDESK,
    HWINSTA,
};

pub(super) struct PrivateDesktop {
    desktop: HDESK,
    pub(super) name: Vec<u16>,
}
impl PrivateDesktop {
    pub(super) fn create(write_sid: &str) -> io::Result<Self> {
        let (owner, _) = current_user_token_identity()?;
        let mut entropy = [0u8; 16];
        getrandom::fill(&mut entropy).map_err(io::Error::other)?;
        let desktop_name =
            format!("AuraR06{}", entropy.iter().map(|b| format!("{b:02x}")).collect::<String>());
        let sddl = wide_null(&format!("D:P(A;;GA;;;{owner})(A;;0x201ff;;;{write_sid})"));
        let mut descriptor = ptr::null_mut();
        // SAFETY: only validated SID values enter SDDL, output is initialized.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let _descriptor = LocalAllocation(descriptor.cast());
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        // Query the current station without changing its ACL or association.
        // CreateDesktopW creates in this station, so the child needs its fully
        // qualified name. A bare desktop name can resolve in another station.
        let station = unsafe { GetProcessWindowStation() };
        if station.is_null() {
            return Err(last_os_error_with_context("R06 GetProcessWindowStation"));
        }
        let station_name = object_name(station)?;
        let mut result = Self {
            desktop: ptr::null_mut(),
            name: wide_null(&format!("{station_name}\\{desktop_name}")),
        };
        result.desktop = unsafe {
            CreateDesktopW(
                wide_null(&desktop_name).as_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                0x1000_0000,
                &attributes,
            )
        };
        let creation_error = last_os_error_with_context("R06 CreateDesktopW");
        if result.desktop.is_null() {
            return Err(creation_error);
        }
        Ok(result)
    }
}

fn object_name(station: HWINSTA) -> io::Result<String> {
    let mut needed = 0;
    // SAFETY: sizing query for UOI_NAME (2), with no output buffer.
    unsafe {
        GetUserObjectInformationW(station, 2, ptr::null_mut(), 0, &mut needed);
    }
    if needed == 0 || needed > 65536 || needed % 2 != 0 {
        return Err(io::Error::other("invalid window station name length"));
    }
    let mut name = vec![0u16; needed as usize / 2];
    // SAFETY: initialized UTF-16 storage covers the returned byte length.
    if unsafe {
        GetUserObjectInformationW(station, 2, name.as_mut_ptr().cast(), needed, &mut needed)
    } == 0
    {
        return Err(last_os_error_with_context("R06 GetUserObjectInformationW"));
    }
    let end = name
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| io::Error::other("unterminated station name"))?;
    String::from_utf16(&name[..end]).map_err(io::Error::other)
}
impl Drop for PrivateDesktop {
    fn drop(&mut self) {
        // SAFETY: owns the non-inherited private desktop; station is borrowed.
        unsafe {
            if !self.desktop.is_null() {
                CloseDesktop(self.desktop);
            }
        }
    }
}
