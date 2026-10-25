use aura_backend_windows_user::compat::environment::{build, excluded_names};
use std::ffi::OsString;

fn source() -> Vec<(OsString, OsString)> {
    [
        ("Path", "C:\\tools"),
        ("GITHUB_TOKEN", "test-value"),
        ("NPM_TOKEN", "test-value"),
        ("MY_SERVICE_API_KEY", "test-value"),
        ("TOKENIZERS_PARALLELISM", "false"),
    ]
    .map(|(name, value)| (name.into(), value.into()))
    .into()
}

#[test]
fn s08_generic_secret_segments_are_removed_without_removing_tokenizers() {
    let entries = build(source(), None).expect("selected development environment");
    for name in ["GITHUB_TOKEN", "NPM_TOKEN", "MY_SERVICE_API_KEY"] {
        assert!(entries.iter().all(|entry| entry.name != name), "secret name {name}");
    }
    assert!(entries.iter().any(|entry| entry.name == "TOKENIZERS_PARALLELISM"));
    assert_eq!(excluded_names(&source()), ["GITHUB_TOKEN", "NPM_TOKEN", "MY_SERVICE_API_KEY"]);
}

#[test]
fn s08_environment_keeps_the_original_name_case() {
    let entries = build(source(), None).expect("selected development environment");
    assert!(entries.iter().any(|entry| entry.name == "Path"));
}

#[test]
fn s08_soft_offline_enables_node_proxy_unless_the_user_selected_it() {
    let proxy = Some("http://127.0.0.1:34567");
    let entries = build(source(), proxy).expect("owned soft offline environment");
    assert!(entries.iter().any(|entry| entry.name == "NODE_USE_ENV_PROXY"
        && String::from_utf16_lossy(&entry.value_utf16) == "1"));
    let entries =
        build([("Node_Use_Env_Proxy".into(), "0".into())], proxy).expect("explicit user choice");
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.name.eq_ignore_ascii_case("NODE_USE_ENV_PROXY"))
            .count(),
        1
    );
    assert!(entries.iter().any(|entry| entry.name == "Node_Use_Env_Proxy"
        && String::from_utf16_lossy(&entry.value_utf16) == "0"));
}
