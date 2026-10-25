use aura_backend_windows_user::compat::environment::build;
use std::ffi::OsString;

fn values(proxy: Option<&str>) -> Vec<aura_windows_platform::NativeEnvironmentEntry> {
    build(
        [
            ("Path", "C:\\tool;;C:\\tool two"),
            ("USERPROFILE", "C:\\fixture"),
            ("AURA_CONTROL_TOKEN", "fixture-only"),
            ("DEEPSEEK_API_KEY", "fixture-only"),
            ("http_proxy", "http://127.0.0.1:1234"),
            ("NO_PROXY", "*"),
        ]
        .map(|(k, v)| (OsString::from(k), OsString::from(v))),
        proxy,
    )
    .expect("trusted child environment")
}

#[test]
fn development_path_and_profile_survive_without_empty_path_segments_or_control_secrets() {
    let values = values(None);
    assert!(
        values.iter().any(|v| v.name == "Path"
            && String::from_utf16_lossy(&v.value_utf16) == "C:\\tool;C:\\tool two")
    );
    assert!(values.iter().any(|v| v.name == "USERPROFILE"));
    assert!(values.iter().all(|v| !v.name.contains("TOKEN") && !v.name.contains("API_KEY")));
}

#[test]
fn offline_proxy_replaces_bypass_but_network_enabled_restores_the_selected_user_proxy() {
    let offline = values(Some("http://127.0.0.1:5678"));
    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
        assert!(offline.iter().any(|v| v.name.eq_ignore_ascii_case(name)
            && String::from_utf16_lossy(&v.value_utf16) == "http://127.0.0.1:5678"));
    }
    assert!(offline.iter().any(|v| v.name == "NO_PROXY" && v.value_utf16.is_empty()));
    assert!(values(None).iter().any(|v| v.name == "http_proxy"
        && String::from_utf16_lossy(&v.value_utf16) == "http://127.0.0.1:1234"));
}

#[test]
fn duplicate_case_names_and_invalid_values_are_refused_without_rewriting_unicode_values() {
    assert!(
        build(
            [("Path", "one"), ("PATH", "two")].map(|(key, value)| (key.into(), value.into())),
            None
        )
        .is_err()
    );
    assert!(build([("VAR".into(), OsString::from("bad\0value"))], None).is_err());
    let entries =
        build([("中文变量".into(), OsString::from("空 格值"))], None).expect("unicode environment");
    assert_eq!(String::from_utf16_lossy(&entries[0].value_utf16), "空 格值");
}
