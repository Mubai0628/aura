//! Read-only device-open diagnostics. Never changes a device security descriptor.
use serde_json::{Value, json};
use std::fs::OpenOptions;

pub fn null_access() -> Value {
    device_access(r"\\.\NUL")
}

pub fn crypto_devices() -> Value {
    json!({"CNG":device_access(r"\\?\GLOBALROOT\Device\CNG"),"KsecDD":device_access(r"\\?\GLOBALROOT\Device\KsecDD")})
}

/// Exact approved KsecDD rights, with device/file ownership rights still denied.
pub fn ksec_boundary_passed(payload: &Value) -> bool {
    payload["ksec_access"]["required"]["opened"] == true
        && ["write_dac", "write_owner", "delete"]
            .iter()
            .all(|right| payload["ksec_access"][right]["opened"] == false)
        && payload["devices"]["CNG"]["read_write"]["opened"] == false
        && payload["devices"]["KsecDD"]["read_write"]["opened"] == false
}

fn device_access(path: &str) -> Value {
    let check = |read, write| match OpenOptions::new().read(read).write(write).open(path) {
        Ok(_) => json!({"opened":true}),
        Err(error) => {
            json!({"opened":false,"os_error":error.raw_os_error(),"kind":format!("{:?}",error.kind())})
        }
    };
    json!({"read":check(true,false),"write":check(false,true),"read_write":check(true,true)})
}

pub fn parent() -> Value {
    let descriptor = match aura_windows_platform::capture_null_device_security_descriptor() {
        Ok(sddl) => json!({"sddl":sddl}),
        Err(error) => json!({"os_error":error.raw_os_error(),"kind":format!("{:?}",error.kind())}),
    };
    json!({"null_access":null_access(),"null_descriptor":descriptor,"crypto_devices":crypto_devices(),"synchronization":aura_windows_platform::spike_synchronization()})
}

pub fn namespace_boundary_passed(payload: &Value) -> bool {
    let access = &payload["namespace_access"];
    access["required"]["opened"] == true
        && ["write_dac", "write_owner", "delete"].iter().all(|key| access[key]["opened"] == false)
        && payload["synchronization"].as_array().is_some_and(|rows| {
            rows.len() == 6
                && [false, true].iter().all(|named| {
                    ["event", "semaphore", "mutex"].iter().all(|kind| {
                        rows.iter().any(|row| {
                            row["named"] == *named
                                && row["kind"] == *kind
                                && row["created"] == true
                                && row["signal"]["ok"] == true
                                && row["wait"]["ok"] == true
                                && row["wait_code"] == 0
                        })
                    })
                })
        })
}

/// Core command readiness keeps every namespace rights negative and actual
/// unnamed synchronization. Local-named IPC remains separately observable.
pub fn core_namespace_boundary_passed(payload: &Value) -> bool {
    let access = &payload["namespace_access"];
    access["required"]["opened"] == true
        && ["write_dac", "write_owner", "delete"].iter().all(|key| access[key]["opened"] == false)
        && payload["synchronization"].as_array().is_some_and(|rows| {
            rows.len() == 6
                && [false, true].iter().all(|named| {
                    ["event", "semaphore", "mutex"].iter().all(|kind| {
                        let matching = rows
                            .iter()
                            .filter(|row| row["named"] == *named && row["kind"] == *kind)
                            .collect::<Vec<_>>();
                        if let [row] = matching.as_slice() {
                            // Named failures stay in the report; never reinterpret
                            // them as a supported capability or missing evidence.
                            if *named {
                                row["created"].is_boolean()
                            } else {
                                row["created"] == true
                                    && row["signal"]["ok"] == true
                                    && row["wait"]["ok"] == true
                                    && row["wait_code"] == 0
                            }
                        } else {
                            false
                        }
                    })
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_requires_actual_operations_and_all_negative_rights() {
        let rows = [false, true].into_iter().flat_map(|named| {
            ["event", "semaphore", "mutex"].into_iter().map(move |kind|
                json!({"named":named,"kind":kind,"created":true,"signal":{"ok":true},"wait":{"ok":true},"wait_code":0}))
        }).collect::<Vec<_>>();
        let passed = json!({"namespace_access":{"required":{"opened":true},"write_dac":{"opened":false},"write_owner":{"opened":false},"delete":{"opened":false}},"synchronization":rows});
        assert!(namespace_boundary_passed(&passed));
        assert!(!namespace_boundary_passed(&json!({})));
        for key in ["write_dac", "write_owner", "delete"] {
            let mut bad = passed.clone();
            bad["namespace_access"][key]["opened"] = json!(true);
            assert!(!namespace_boundary_passed(&bad));
        }
        let mut duplicate = passed.clone();
        duplicate["synchronization"][5] = duplicate["synchronization"][0].clone();
        assert!(!namespace_boundary_passed(&duplicate));
        let mut wait = passed;
        wait["synchronization"][3]["wait_code"] = json!(258);
        assert!(!namespace_boundary_passed(&wait));
    }
    #[test]
    fn core_readiness_does_not_promote_named_ipc_or_ignore_security_negatives() {
        let rows = [false,true].into_iter().flat_map(|named| ["event","semaphore","mutex"].into_iter()
            .map(move |kind| json!({"named":named,"kind":kind,"created":!named,"signal":{"ok":!named},"wait":{"ok":!named},"wait_code":0}))).collect::<Vec<_>>();
        let good = json!({"namespace_access":{"required":{"opened":true},"write_dac":{"opened":false},"write_owner":{"opened":false},"delete":{"opened":false}},"synchronization":rows});
        assert!(core_namespace_boundary_passed(&good));
        assert!(!namespace_boundary_passed(&good));
        for key in ["write_dac", "write_owner", "delete"] {
            let mut bad = good.clone();
            bad["namespace_access"][key]["opened"] = json!(true);
            assert!(!core_namespace_boundary_passed(&bad));
        }
        let mut missing = good.clone();
        missing["synchronization"][3] = json!({});
        assert!(!core_namespace_boundary_passed(&missing));
        let mut bad = good;
        bad["synchronization"][0]["wait_code"] = json!(258);
        assert!(!core_namespace_boundary_passed(&bad));
    }
    #[test]
    fn exact_access_requires_negative_boundaries_and_complete_evidence() {
        let passed = json!({"ksec_access":{"required":{"opened":true},
            "write_dac":{"opened":false},"write_owner":{"opened":false},"delete":{"opened":false}},
            "devices":{"CNG":{"read_write":{"opened":false}},"KsecDD":{"read_write":{"opened":false}}}});
        assert!(ksec_boundary_passed(&passed));
        assert!(!ksec_boundary_passed(&json!({})));
        for right in ["write_dac", "write_owner", "delete"] {
            let mut expanded = passed.clone();
            expanded["ksec_access"][right]["opened"] = json!(true);
            assert!(!ksec_boundary_passed(&expanded));
        }
        let mut cng = passed.clone();
        cng["devices"]["CNG"]["read_write"]["opened"] = json!(true);
        assert!(!ksec_boundary_passed(&cng));
        let mut broad = passed;
        broad["devices"]["KsecDD"]["read_write"]["opened"] = json!(true);
        assert!(!ksec_boundary_passed(&broad));
    }
}
