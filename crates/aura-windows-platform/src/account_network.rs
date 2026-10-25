//! Fixed per-account WFP primitives. Ownership keys and existing receipts keep
//! their original meanings; production assurance is versioned by the Backend.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{io, ptr};
use windows_sys::{
    Win32::{
        Foundation::HANDLE, NetworkManagement::WindowsFilteringPlatform::*,
        Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW,
    },
    core::GUID,
};

mod objects;
#[cfg(feature = "restricted-user-spike")]
mod rpc_experiment;
#[cfg(feature = "restricted-user-spike")]
pub use rpc_experiment::{inspect_spike_rpc_guard, spike_rpc_guard_plan};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountNetworkPolicy {
    BlockAll,
    AllowLoopback,
}

pub struct AccountNetworkSpec {
    pub install_id: String,
    pub offline_sid: String,
    pub policy: AccountNetworkPolicy,
    pub expected_sublayer_weight: Option<u16>,
}
impl AccountNetworkSpec {
    fn validate(&self) -> io::Result<()> {
        let id = self.install_id.as_bytes();
        if id.len() != 36
            || !id.iter().enumerate().all(|(i, c)| {
                if [8, 13, 18, 23].contains(&i) {
                    *c == b'-'
                } else {
                    c.is_ascii_hexdigit() && !c.is_ascii_uppercase()
                }
            })
        {
            return Err(io::Error::other("noncanonical installation UUID"));
        }
        let tail = self
            .offline_sid
            .strip_prefix("S-1-5-21-")
            .ok_or_else(|| io::Error::other("dedicated local account SID required"))?;
        let fields = tail.split('-').collect::<Vec<_>>();
        if fields.len() != 4
            || fields
                .iter()
                .any(|v| v.parse::<u32>().ok().is_none_or(|n| n == 0 || n.to_string() != *v))
        {
            return Err(io::Error::other("invalid local account SID"));
        }
        Ok(())
    }
    fn key(&self, purpose: &str) -> GUID {
        let hash = Sha256::digest(format!("Aura.R06.WFP.v1/{}/{purpose}", self.install_id));
        GUID::from_u128(u128::from_be_bytes(hash[..16].try_into().expect("fixed digest")))
    }
    fn marker(&self) -> Vec<u8> {
        format!("Aura.R06.WFP.v1/{}/{}/{:?}", self.install_id, self.offline_sid, self.policy)
            .into_bytes()
    }
}

const LAYERS: [(GUID, &str); 4] = [
    (FWPM_LAYER_ALE_AUTH_CONNECT_V4, "connect-v4"),
    (FWPM_LAYER_ALE_AUTH_CONNECT_V6, "connect-v6"),
    (FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V4, "accept-v4"),
    (FWPM_LAYER_ALE_AUTH_RECV_ACCEPT_V6, "accept-v6"),
];

/// Operations are fixed; callers must bind the spec to the installed account.
/// A failed transaction is aborted, including partially added/deleted objects.
pub fn manage_account_network(spec: &AccountNetworkSpec, operation: &str) -> io::Result<Value> {
    spec.validate()?;
    if operation == "plan" {
        return Ok(
            json!({"schema":"r06-wfp-v1","policy":match spec.policy {AccountNetworkPolicy::BlockAll=>"block_all",AccountNetworkPolicy::AllowLoopback=>"allow_loopback"},"sublayer_key":format!("{:032x}",guid_value(spec.key("sublayer"))),"filters":LAYERS.iter().map(|(_,purpose)|json!({"purpose":purpose,"key":format!("{:032x}",guid_value(spec.key(purpose)))})).collect::<Vec<_>>(),"persistent":true,"custom_driver":false,"custom_service":false,"ready":false}),
        );
    }
    if !["inspect", "install", "remove"].contains(&operation) {
        return Err(io::Error::other("unknown WFP operation"));
    }
    if operation != "inspect" && !crate::current_process_is_elevated()? {
        return Err(io::Error::other("explicit administrator shell required for WFP mutation"));
    }
    let engine = Engine::open()?;
    // Default BFE grants ordinary users OPEN, not BEGIN_READ_TXN. Inspection
    // reads named objects using their explicit read ACL; it is a snapshot only.
    let mut transaction =
        if operation == "inspect" { None } else { Some(Transaction::begin(&engine, false)?) };
    let marker = spec.marker();
    let mut name = wide(&format!("Aura R06 WFP {}", spec.install_id));
    let subkey = spec.key("sublayer");
    let sublayer = FWPM_SUBLAYER0 {
        subLayerKey: subkey,
        displayData: FWPM_DISPLAY_DATA0 { name: name.as_mut_ptr(), description: ptr::null_mut() },
        flags: FWPM_SUBLAYER_FLAG_PERSISTENT,
        providerData: blob(&marker),
        weight: 0x8000,
        ..Default::default()
    };
    // No provider/service association: persistent filters must not depend on a
    // nonexistent auto-start provider service after BFE restarts.
    let object_sd =
        Descriptor::new("O:BAG:BAD:P(A;;0x000f07ff;;;SY)(A;;0x000f07ff;;;BA)(A;;0x00020080;;;WD)")?;
    let user_sd = Descriptor::new(&format!("D:(A;;CC;;;{})", spec.offline_sid))?;
    let mut user_blob = user_sd.blob();
    let mut conditions = vec![FWPM_FILTER_CONDITION0 {
        fieldKey: FWPM_CONDITION_ALE_USER_ID,
        matchType: FWP_MATCH_EQUAL,
        conditionValue: FWP_CONDITION_VALUE0 {
            r#type: FWP_SECURITY_DESCRIPTOR_TYPE,
            Anonymous: FWP_CONDITION_VALUE0_0 { sd: &mut user_blob },
        },
    }];
    if spec.policy == AccountNetworkPolicy::AllowLoopback {
        conditions.push(FWPM_FILTER_CONDITION0 {
            fieldKey: FWPM_CONDITION_FLAGS,
            matchType: FWP_MATCH_FLAGS_NONE_SET,
            conditionValue: FWP_CONDITION_VALUE0 {
                r#type: FWP_UINT32,
                Anonymous: FWP_CONDITION_VALUE0_0 { uint32: FWP_CONDITION_FLAG_IS_LOOPBACK },
            },
        });
    }
    let mut assigned_weight =
        engine.sublayer(&sublayer, &object_sd, spec.expected_sublayer_weight)?;
    let sub_exists = assigned_weight.is_some();
    if operation == "install" && !sub_exists {
        check(unsafe { FwpmSubLayerAdd0(engine.0, &sublayer, object_sd.0) }, "add sublayer")?;
        // BFE chooses the nearest available weight. Capture the assigned value
        // in this write transaction, then pin it in the installation receipt.
        assigned_weight = engine.sublayer(&sublayer, &object_sd, None)?;
    }
    let mut found = 0;
    let mut keys = Vec::new();
    for (layer, purpose) in LAYERS {
        let key = spec.key(purpose);
        let filter = FWPM_FILTER0 {
            filterKey: key,
            displayData: sublayer.displayData,
            flags: FWPM_FILTER_FLAG_PERSISTENT,
            providerData: blob(&marker),
            layerKey: layer,
            subLayerKey: subkey,
            weight: FWP_VALUE0 { r#type: FWP_UINT8, Anonymous: FWP_VALUE0_0 { uint8: 15 } },
            numFilterConditions: conditions.len() as u32,
            filterCondition: conditions.as_mut_ptr(),
            action: FWPM_ACTION0 { r#type: FWP_ACTION_BLOCK, ..Default::default() },
            ..Default::default()
        };
        let exists = engine.filter(&filter, &object_sd)?;
        if exists {
            found += 1;
        }
        match operation {
            "install" if !exists => check(
                unsafe { FwpmFilterAdd0(engine.0, &filter, object_sd.0, ptr::null_mut()) },
                "add filter",
            )?,
            "remove" if exists => {
                check(unsafe { FwpmFilterDeleteByKey0(engine.0, &key) }, "delete owned filter")?
            }
            _ => (),
        }
        keys.push(json!({"purpose":purpose,"key":format!("{:032x}",guid_value(key)),"present_before":exists}));
    }
    if operation == "remove" && sub_exists {
        // Foreign filters linked into our sublayer make deletion fail; rollback
        // restores our filters rather than deleting anybody else's objects.
        check(unsafe { FwpmSubLayerDeleteByKey0(engine.0, &subkey) }, "delete owned sublayer")?;
    }
    if let Some(transaction) = transaction.as_mut() {
        transaction.commit()?;
    }
    Ok(
        json!({"schema":"r06-wfp-v1","operation":operation,"policy":match spec.policy {AccountNetworkPolicy::BlockAll=>"block_all",AccountNetworkPolicy::AllowLoopback=>"allow_loopback"},
        "sublayer_key":format!("{:032x}",guid_value(subkey)),"sublayer_weight":assigned_weight,"filters":keys,"ready":operation=="install"||(operation=="inspect"&&sub_exists&&found==4),
        "network_enforcement":"requires_real_probes","r06_complete":false}),
    )
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain([0]).collect()
}
fn guid_value(value: GUID) -> u128 {
    (u128::from(value.data1) << 96)
        | (u128::from(value.data2) << 80)
        | (u128::from(value.data3) << 64)
        | u128::from(u64::from_be_bytes(value.data4))
}
fn blob(bytes: &[u8]) -> FWP_BYTE_BLOB {
    FWP_BYTE_BLOB { size: bytes.len() as u32, data: bytes.as_ptr().cast_mut() }
}
fn check(code: u32, context: &str) -> io::Result<()> {
    if code == 0 { Ok(()) } else { Err(io::Error::other(format!("WFP {context}: 0x{code:08x}"))) }
}
struct Engine(HANDLE);
impl Engine {
    fn open() -> io::Result<Self> {
        let mut handle = ptr::null_mut();
        check(
            unsafe { FwpmEngineOpen0(ptr::null(), 10, ptr::null(), ptr::null(), &mut handle) },
            "open engine",
        )?;
        Ok(Self(handle))
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { FwpmEngineClose0(self.0) };
    }
}
struct Transaction<'a> {
    engine: &'a Engine,
    committed: bool,
}
impl<'a> Transaction<'a> {
    fn begin(engine: &'a Engine, readonly: bool) -> io::Result<Self> {
        check(
            unsafe {
                FwpmTransactionBegin0(engine.0, if readonly { FWPM_TXN_READ_ONLY } else { 0 })
            },
            "begin transaction",
        )?;
        Ok(Self { engine, committed: false })
    }
    fn commit(&mut self) -> io::Result<()> {
        check(unsafe { FwpmTransactionCommit0(self.engine.0) }, "commit transaction")?;
        self.committed = true;
        Ok(())
    }
}
impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.committed {
            unsafe { FwpmTransactionAbort0(self.engine.0) };
        }
    }
}
struct Descriptor(*mut std::ffi::c_void, u32);
impl Descriptor {
    fn new(sddl: &str) -> io::Result<Self> {
        let mut sd = ptr::null_mut();
        let mut len = 0;
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl).as_ptr(),
                1,
                &mut sd,
                &mut len,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(sd, len))
    }
    fn blob(&self) -> FWP_BYTE_BLOB {
        FWP_BYTE_BLOB { size: self.1, data: self.0.cast() }
    }
}
impl Drop for Descriptor {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::LocalFree(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn spec() -> AccountNetworkSpec {
        AccountNetworkSpec {
            install_id: "073773f9-1798-4a46-a6d1-b8d5c9e3d19a".into(),
            offline_sid: "S-1-5-21-1-2-3-1001".into(),
            policy: AccountNetworkPolicy::BlockAll,
            expected_sublayer_weight: None,
        }
    }
    #[test]
    fn rejects_broad_or_noncanonical_principals() {
        for sid in ["S-1-1-0", "S-1-5-18", "S-1-5-21-1-2-3", "S-1-5-21-1-2-3-01"] {
            let mut s = spec();
            s.offline_sid = sid.into();
            assert!(s.validate().is_err());
        }
    }
    #[test]
    fn ownership_keys_survive_policy_change_but_marker_detects_drift() {
        let s = spec();
        assert!(s.validate().is_ok());
        let mut changed = spec();
        changed.policy = AccountNetworkPolicy::AllowLoopback;
        assert_eq!(guid_value(s.key("connect-v4")), guid_value(changed.key("connect-v4")));
        assert_ne!(s.marker(), changed.marker());
        assert_ne!(guid_value(s.key("connect-v4")), guid_value(s.key("accept-v4")));
    }
}
