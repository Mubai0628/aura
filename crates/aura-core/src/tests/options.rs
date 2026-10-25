use super::*;

#[test]
fn parses_and_validates_a_secure_pipe_name() {
    let runtime = RuntimeInstanceId::new();
    let pipe = format!(
        r"\\.\pipe\Aura\a47182cd8b9177c29a6be19b0c9c32a4\{runtime}\00112233445566778899aabbccddeeff"
    );
    let options = CoreOptions::parse([
        "aura-core.exe".to_owned(),
        format!("--pipe-name={pipe}"),
        "--bootstrap-stdin".to_owned(),
        "--parent-pid=42".to_owned(),
        format!("--runtime-instance-id={runtime}"),
        "--component-manifest=C:\\Aura\\component-manifest.json".to_owned(),
        "--allow-dev-component-mismatch".to_owned(),
        "--test-mode=incompatible".to_owned(),
    ])
    .expect("valid Core options");
    assert_eq!(options.test_mode, CoreTestMode::Incompatible);
    assert!(options.allow_dev_component_mismatch);
}

#[test]
fn rejects_pipe_names_without_128_bits_of_randomness() {
    let runtime = RuntimeInstanceId::new();
    let result = CoreOptions::parse([
        "aura-core.exe".to_owned(),
        format!(r"--pipe-name=\\.\pipe\Aura\a47182cd8b9177c29a6be19b0c9c32a4\{runtime}\short"),
        "--bootstrap-stdin".to_owned(),
        "--parent-pid=42".to_owned(),
        format!("--runtime-instance-id={runtime}"),
        "--component-manifest=manifest.json".to_owned(),
    ]);
    assert!(result.is_err());
}
