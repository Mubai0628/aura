//! Retired broad RPC experiment: historical plan and read-only inspection only.
//! No persistent policy change, service mutation, driver or Core registration.
use super::*;

pub fn spike_rpc_guard_plan(spec: &AccountNetworkSpec) -> io::Result<Value> {
    spec.validate()?;
    Ok(json!({
        "schema":"r06-rpc-guard-plan-v1", "install_id":spec.install_id,
        "offline_sid":spec.offline_sid, "layer":"FWPM_LAYER_RPC_UM",
        "conditions":{"remote_user_token":spec.offline_sid,"rpc_protocol":"LRPC"},
        "filter_key":format!("{:032x}",guid_value(spec.key("rpc-experiment-filter"))),
        "sublayer_key":format!("{:032x}",guid_value(spec.key("rpc-experiment-sublayer"))),
        "action":"block", "dynamic_session":true,"persistent":false,
        "maximum_helper_seconds":180,"scope":"all local RPC for the dedicated Offline identity only",
        "compatibility":"unverified; may prevent shell startup or Windows service use",
        "production_ready":false,"r06_complete":false,"retired":true,"execution_allowed":false
    }))
}

/// Read by exact keys after session closure. Presence is NOT proof that a
/// filter has the right policy, and is never used for production readiness.
pub fn inspect_spike_rpc_guard(spec: &AccountNetworkSpec) -> io::Result<Value> {
    spec.validate()?;
    let engine = Engine::open()?;
    let mut filter = ptr::null_mut();
    let code =
        unsafe { FwpmFilterGetByKey0(engine.0, &spec.key("rpc-experiment-filter"), &mut filter) };
    let filter_present = if code == 0 {
        unsafe { FwpmFreeMemory0((&mut filter as *mut *mut FWPM_FILTER0).cast()) };
        true
    } else if code == windows_sys::Win32::Foundation::FWP_E_FILTER_NOT_FOUND as u32 {
        false
    } else {
        check(code, "inspect RPC filter")?;
        false
    };
    let mut sublayer = ptr::null_mut();
    let code = unsafe {
        FwpmSubLayerGetByKey0(engine.0, &spec.key("rpc-experiment-sublayer"), &mut sublayer)
    };
    let sublayer_present = if code == 0 {
        unsafe { FwpmFreeMemory0((&mut sublayer as *mut *mut FWPM_SUBLAYER0).cast()) };
        true
    } else if code == windows_sys::Win32::Foundation::FWP_E_SUBLAYER_NOT_FOUND as u32 {
        false
    } else {
        check(code, "inspect RPC sublayer")?;
        false
    };
    Ok(
        json!({"schema":"r06-rpc-guard-inspection-v1","filter_present":filter_present,"sublayer_present":sublayer_present,"r06_complete":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_plan_cannot_authorize_execution_or_persistent_network_policy() {
        let mut spec = AccountNetworkSpec {
            install_id: "073773f9-1798-4a46-a6d1-b8d5c9e3d19a".into(),
            offline_sid: "S-1-5-21-1-2-3-1001".into(),
            policy: AccountNetworkPolicy::BlockAll,
            expected_sublayer_weight: None,
        };
        let p = spike_rpc_guard_plan(&spec).expect("historical plan remains non-executable");
        assert_eq!(p["conditions"]["rpc_protocol"], "LRPC");
        assert_eq!(p["conditions"]["remote_user_token"], spec.offline_sid);
        assert_eq!(p["persistent"], false);
        assert_eq!(p["retired"], true);
        assert_eq!(p["execution_allowed"], false);
        assert_eq!(p["maximum_helper_seconds"], 180);
        assert_ne!(spec.key("rpc-experiment-filter").data1, spec.key("connect-v4").data1);
        spec.offline_sid = "S-1-1-0".into();
        assert!(spike_rpc_guard_plan(&spec).is_err());
    }
}
