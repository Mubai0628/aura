//! Reconciliation refuses collisions and drift before any transaction commits.
use super::*;
use windows_sys::Win32::{
    Foundation::{FWP_E_FILTER_NOT_FOUND, FWP_E_SUBLAYER_NOT_FOUND, LocalFree},
    Security::Authorization::ConvertSecurityDescriptorToStringSecurityDescriptorW,
};

struct Allocation<T>(*mut T);
impl<T> Drop for Allocation<T> {
    fn drop(&mut self) {
        unsafe { FwpmFreeMemory0((&raw mut self.0).cast()) };
    }
}
fn same_guid(a: GUID, b: GUID) -> bool {
    guid_value(a) == guid_value(b)
}
fn bytes(blob: &FWP_BYTE_BLOB) -> &[u8] {
    if blob.size == 0 || blob.data.is_null() {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(blob.data, blob.size as usize) }
    }
}
fn require(ok: bool, field: &str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "WFP owned object mismatch ({field}); no overwrite or deletion"
        )))
    }
}

impl Engine {
    pub(super) fn sublayer(
        &self,
        expected: &FWPM_SUBLAYER0,
        sd: &Descriptor,
        pinned_weight: Option<u16>,
    ) -> io::Result<Option<u16>> {
        let mut found = ptr::null_mut();
        let code = unsafe { FwpmSubLayerGetByKey0(self.0, &expected.subLayerKey, &mut found) };
        if code == FWP_E_SUBLAYER_NOT_FOUND as u32 {
            return Ok(None);
        }
        check(code, "query sublayer")?;
        let value = Allocation(found);
        let actual = unsafe { &*value.0 };
        require(
            actual.flags == expected.flags
                && actual.providerKey.is_null()
                && bytes(&actual.providerData) == bytes(&expected.providerData),
            "sublayer flags/provider/ownership marker",
        )?;
        verify_weight(actual.weight, pinned_weight)?;
        self.security(&expected.subLayerKey, false, sd)?;
        Ok(Some(actual.weight))
    }
    pub(super) fn filter(&self, expected: &FWPM_FILTER0, sd: &Descriptor) -> io::Result<bool> {
        let mut found = ptr::null_mut();
        let code = unsafe { FwpmFilterGetByKey0(self.0, &expected.filterKey, &mut found) };
        if code == FWP_E_FILTER_NOT_FOUND as u32 {
            return Ok(false);
        }
        check(code, "query filter")?;
        let value = Allocation(found);
        let actual = unsafe { &*value.0 };
        require(
            actual.flags == expected.flags
                && actual.providerKey.is_null()
                && actual.action.r#type == FWP_ACTION_BLOCK
                && same_guid(actual.layerKey, expected.layerKey)
                && same_guid(actual.subLayerKey, expected.subLayerKey)
                && bytes(&actual.providerData) == bytes(&expected.providerData)
                && actual.weight.r#type == FWP_UINT8
                && unsafe { actual.weight.Anonymous.uint8 } == 15
                && actual.numFilterConditions == expected.numFilterConditions
                && !actual.filterCondition.is_null(),
            "filter flags/provider/action/layer/marker/weight/condition count",
        )?;
        let actual_conditions = unsafe {
            std::slice::from_raw_parts(actual.filterCondition, actual.numFilterConditions as usize)
        };
        let expected_conditions = unsafe {
            std::slice::from_raw_parts(
                expected.filterCondition,
                expected.numFilterConditions as usize,
            )
        };
        // BFE may reorder conditions; match by unique field rather than position.
        for condition in expected_conditions {
            let matches = actual_conditions
                .iter()
                .filter(|v| same_guid(v.fieldKey, condition.fieldKey))
                .collect::<Vec<_>>();
            require(matches.len() == 1, "condition field uniqueness")?;
            let other = matches[0];
            require(
                other.matchType == condition.matchType
                    && other.conditionValue.r#type == condition.conditionValue.r#type,
                "condition match/value type",
            )?;
            if condition.conditionValue.r#type == FWP_UINT32 {
                require(
                    unsafe {
                        other.conditionValue.Anonymous.uint32
                            == condition.conditionValue.Anonymous.uint32
                    },
                    "condition flags",
                )?;
            } else {
                let actual_sd = unsafe { other.conditionValue.Anonymous.sd };
                require(!actual_sd.is_null(), "condition descriptor presence")?;
                require(
                    unsafe { bytes(&*actual_sd) == bytes(&*condition.conditionValue.Anonymous.sd) },
                    "condition account descriptor",
                )?;
            }
        }
        self.security(&expected.filterKey, true, sd)?;
        Ok(true)
    }
    fn security(&self, key: &GUID, filter: bool, expected: &Descriptor) -> io::Result<()> {
        let mut found = ptr::null_mut();
        let code = unsafe {
            if filter {
                FwpmFilterGetSecurityInfoByKey0(
                    self.0,
                    key,
                    7,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut found,
                )
            } else {
                FwpmSubLayerGetSecurityInfoByKey0(
                    self.0,
                    key,
                    7,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut found,
                )
            }
        };
        check(code, "query object owner/group/DACL")?;
        let value = Allocation(found);
        require(
            security_text(value.0)? == security_text(expected.0)?,
            "object owner/group/protected DACL",
        )
    }
}
fn security_text(sd: *mut std::ffi::c_void) -> io::Result<String> {
    let mut string = ptr::null_mut();
    let mut len = 0;
    if unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(sd, 1, 7, &mut string, &mut len)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let result = unsafe { std::slice::from_raw_parts(string, len as usize) }.to_vec();
    unsafe { LocalFree(string.cast()) };
    // AI records completed auto-inheritance. With a protected DACL, it changes
    // neither ACEs nor access. Keep P, owner/group and every ACE unchanged.
    Ok(normalize_security(&String::from_utf16_lossy(&result)))
}

fn normalize_security(text: &str) -> String {
    text.replace("D:PAI(", "D:P(")
}
fn verify_weight(actual: u16, pinned: Option<u16>) -> io::Result<()> {
    require(pinned.is_none_or(|expected| expected == actual), "recorded sublayer weight")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepts_auto_inherited_metadata_but_rejects_security_changes() {
        let expected =
            Descriptor::new("O:BAG:BAD:P(A;;0xf07ff;;;SY)(A;;0xf07ff;;;BA)(A;;0x20080;;;WD)")
                .expect("SD");
        let returned =
            Descriptor::new("O:BAG:BAD:PAI(A;;0xf07ff;;;SY)(A;;0xf07ff;;;BA)(A;;0x20080;;;WD)")
                .expect("SD");
        assert_eq!(
            security_text(expected.0).expect("SDDL"),
            security_text(returned.0).expect("SDDL")
        );
        for changed in [
            "O:BAG:BAD:PAI(A;;0xf07ff;;;WD)",
            "O:BAG:BAD:AI(A;;0xf07ff;;;SY)(A;;0xf07ff;;;BA)(A;;0x20080;;;WD)",
            "O:SYG:BAD:PAI(A;;0xf07ff;;;SY)(A;;0xf07ff;;;BA)(A;;0x20080;;;WD)",
        ] {
            let sd = Descriptor::new(changed).expect("changed SD");
            assert_ne!(
                security_text(expected.0).expect("SDDL"),
                security_text(sd.0).expect("SDDL")
            );
        }
    }
    #[test]
    fn assigned_weight_is_recorded_then_changes_are_rejected() {
        assert!(verify_weight(32771, None).is_ok());
        assert!(verify_weight(32771, Some(32771)).is_ok());
        assert!(verify_weight(32772, Some(32771)).is_err());
    }
}
