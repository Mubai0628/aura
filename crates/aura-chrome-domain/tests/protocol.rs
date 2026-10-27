use aura_chrome_domain::commands::*;
use aura_chrome_domain::protocol::*;

/// The samples the extension's tests read too: each is exactly one Rust shape, field for field.
#[test]
fn the_shared_fixture_is_the_wire_shape() {
    fn same<T: serde::de::DeserializeOwned + serde::Serialize>(value: &serde_json::Value) {
        let parsed: T = serde_json::from_value(value.clone())
            .unwrap_or_else(|error| panic!("{value}: {error}"));
        assert_eq!(&serde_json::to_value(parsed).expect("json"), value);
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/chrome/protocol.v1.json"))
            .expect("fixture");
    let ops = fixture["commands"].as_array().expect("commands");
    for command in ops {
        same::<ChromeCommand>(command);
    }
    let named = ops.iter().filter_map(|command| command["op"].as_str());
    assert_eq!(named.collect::<std::collections::BTreeSet<_>>().len(), 16, "every command");
    for message in fixture["core"].as_array().expect("core") {
        same::<CoreMessage>(message);
    }
    for message in fixture["extension"].as_array().expect("extension") {
        same::<ExtensionMessage>(message);
    }
    let reports = &fixture["reports"];
    same::<ChromeInspection>(&reports["inspection"]);
    same::<ChromePageReport>(&reports["page"]);
    same::<ChromeOpenReport>(&reports["open"]);
    same::<ChromeScreenshotReport>(&reports["screenshot"]);
    same::<ChromeConsoleReport>(&reports["console"]);
    same::<ChromeAssistReport>(&reports["assist"]);
    same::<ChromeCloseReport>(&reports["close"]);
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// RFC 4231 test cases 1, 2 and 6 (a key longer than a block).
#[test]
fn hmac_sha256_matches_the_rfc_vectors() {
    assert_eq!(
        hex(&hmac_sha256(&[0x0b; 20], b"Hi There")),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
    assert_eq!(
        hex(&hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert_eq!(
        hex(&hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
        "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
    );
}

#[test]
fn the_mac_binds_the_challenge_the_extension_and_the_protocol() {
    let secret = [7_u8; PAIRING_SECRET_BYTES];
    let mac = authentication_mac(&secret, b"nonce-one", "abcdefghijklmnopabcdefghijklmnop");
    assert_ne!(mac, authentication_mac(&secret, b"nonce-two", "abcdefghijklmnopabcdefghijklmnop"));
    assert_ne!(mac, authentication_mac(&secret, b"nonce-one", "ponmlkjihgfedcbaponmlkjihgfedcba"));
    assert_ne!(mac, authentication_mac(&[8; 32], b"nonce-one", "abcdefghijklmnopabcdefghijklmnop"));
    assert!(constant_time_eq(&mac, &mac.clone()));
    assert!(!constant_time_eq(&mac, &mac[..31]));
    // The extension's test pins the same proof (computed independently with Node's HMAC).
    assert_eq!(
        hex(&authentication_mac(&secret, b"nonce-one", TEST_EXTENSION_ID)),
        "1ab19d4b8cbbeb8af9b05e05c7035e978f3b36b596eb988821f6ff0950fa223a"
    );
}

#[test]
fn messages_have_one_wire_shape() {
    let hello: ExtensionMessage = serde_json::from_str(
        r#"{"type":"hello","extension_id":"abcdefghijklmnopabcdefghijklmnop","extension_version":"1.0.0"}"#,
    )
    .expect("hello");
    assert!(matches!(hello, ExtensionMessage::Hello { .. }));
    let refused =
        serde_json::to_value(CoreMessage::PairRefused { reason: PairRefusal::CodeExpired })
            .expect("json");
    assert_eq!(refused, serde_json::json!({"type":"pair_refused","reason":"code_expired"}));
    let outcome = serde_json::json!({"type":"result","id":3,"outcome":{"status":"done","value":1}});
    assert!(serde_json::from_value::<ExtensionMessage>(outcome).is_ok());
    assert!(
        serde_json::from_str::<ExtensionMessage>(r#"{"type":"pair","code":"1","extra":1}"#)
            .is_err()
    );
}

#[test]
fn only_an_extension_origin_names_an_extension() {
    let id = "abcdefghijklmnopabcdefghijklmnop";
    assert_eq!(extension_id_of_origin(&format!("chrome-extension://{id}/")), Some(id));
    for origin in [
        "https://example.com/".to_owned(),
        format!("chrome-extension://{id}"),
        "chrome-extension://short/".to_owned(),
        format!("chrome-extension://{}/", "z".repeat(32)),
    ] {
        assert_eq!(extension_id_of_origin(&origin), None, "{origin}");
    }
}

#[test]
fn a_test_namespace_gets_its_own_host_name_and_the_test_extension() {
    assert_eq!(native_host_name(PRODUCTION_NAMESPACE), "com.aura.chrome_control");
    assert_eq!(extension_id_for(PRODUCTION_NAMESPACE), PRODUCTION_EXTENSION_ID);
    let test = native_host_name("aura.desktop.test.chrome");
    assert!(test.starts_with("com.aura.chrome_control.t") && test.len() == 37, "{test}");
    assert_ne!(test, native_host_name("aura.desktop.test.other"));
    // scripts/release/chrome-extension.test.mjs pins the same value for the extension's build.
    assert_eq!(native_host_name("aura.desktop.test.b6"), "com.aura.chrome_control.t2fbad0b3fbc4");
    assert_eq!(extension_id_for("aura.desktop.test.chrome"), TEST_EXTENSION_ID);
    for id in [PRODUCTION_EXTENSION_ID, TEST_EXTENSION_ID] {
        assert_eq!(extension_id_of_origin(&format!("chrome-extension://{id}/")), Some(id));
    }
}

#[test]
fn a_pipe_is_named_by_its_installation_folder_whatever_the_case() {
    let one = pipe_name(std::path::Path::new(r"C:\Users\Me\AppData\Local\Aura\versions\0.1.4"));
    let same = pipe_name(std::path::Path::new(r"c:\users\me\appdata\local\aura\versions\0.1.4"));
    let other = pipe_name(std::path::Path::new(r"C:\Users\Me\AppData\Local\Aura\versions\0.1.5"));
    assert_eq!(one, same);
    assert_ne!(one, other);
    assert!(one.starts_with(r"\\.\pipe\aura-chrome-"), "{one}");
}

/// A canonical path is compared in the form Windows reports a running program's image in.
#[test]
fn a_canonical_path_loses_only_its_verbatim_prefix() {
    let plain = |text: &str| plain_path(std::path::Path::new(text)).to_string_lossy().into_owned();
    assert_eq!(
        plain(r"\\?\C:\Program Files\Aura\aura-core.exe"),
        r"C:\Program Files\Aura\aura-core.exe"
    );
    assert_eq!(
        plain(r"\\?\UNC\server\share\Aura\aura-core.exe"),
        r"\\server\share\Aura\aura-core.exe"
    );
    assert_eq!(plain(r"C:\Aura\aura-core.exe"), r"C:\Aura\aura-core.exe");
    assert_eq!(
        plain(r"\\?\Volume{0b1c}\Aura\aura-core.exe"),
        r"\\?\Volume{0b1c}\Aura\aura-core.exe"
    );
}
