use aura_secret_scanner::detect_text;
#[test]
fn shared_detector_keeps_the_existing_network_patterns_and_returns_only_a_class() {
    for (value, kind) in [
        ("sk-abcdefghijklmnopqrstuvwxyz012345", "api_token"),
        ("ghp_abcdefghijklmnopqrstuvwxyz0123", "github_token"),
        ("AKIAABCDEFGHIJKLMNOP", "aws_access_key"),
        ("eyJabcdefghijk.abcdefghijk.abcdefghijk", "jwt"),
        ("password=temporary-test-value", "credential_assignment"),
        ("-----BEGIN PRIVATE KEY-----\nsynthetic\n-----END PRIVATE KEY-----", "private_key"),
    ] {
        assert_eq!(detect_text(value), Some(kind));
        assert!(!kind.contains(value));
    }
    assert_eq!(detect_text("# 构建约定\ncargo test --offline"), None);
}
