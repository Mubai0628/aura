use super::super::*;
use std::ffi::OsString;

#[test]
fn program_environment_filters_inherited_secrets_and_keeps_shell_network_semantics() {
    let root = tempfile::tempdir().expect("temporary environment");
    let block = || {
        [
            (OsString::from("TEST_SECRET"), OsString::from("temporary-secret")),
            ("OPENAI_API_KEY".into(), "temporary-key".into()),
            ("AURA_INTERNAL_TEST".into(), "temporary-internal".into()),
            ("PATH".into(), "C:\\fixture;;C:\\other".into()),
            ("HTTP_PROXY".into(), "http://127.0.0.1:9".into()),
        ]
    };
    for online in [false, true] {
        let environment = ProgramEnvironment::from_block(
            block(),
            &[("PACKAGE_MESSAGE".into(), "mapped".into())],
            online,
            root.path(),
        )
        .expect("build");
        let get = |name: &str| {
            environment
                .entries()
                .iter()
                .find(|entry| entry.name.eq_ignore_ascii_case(name))
                .map(|entry| String::from_utf16(&entry.value_utf16).expect("synthetic UTF16"))
        };
        assert!(
            get("TEST_SECRET").is_none()
                && get("OPENAI_API_KEY").is_none()
                && get("AURA_INTERNAL_TEST").is_none()
        );
        assert_eq!(get("PACKAGE_MESSAGE").as_deref(), Some("mapped"));
        assert_eq!(get("TEMP").as_deref(), root.path().to_str());
        assert_eq!(get("PATH").as_deref(), Some("C:\\fixture;C:\\other"));
        if online {
            assert_eq!(get("HTTP_PROXY").as_deref(), Some("http://127.0.0.1:9"));
        } else {
            assert!(
                get("HTTP_PROXY").expect("owned reject proxy").starts_with("http://127.0.0.1:")
            );
            assert_ne!(get("HTTP_PROXY").as_deref(), Some("http://127.0.0.1:9"));
        }
    }
}
