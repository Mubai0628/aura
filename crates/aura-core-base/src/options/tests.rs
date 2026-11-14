use super::*;

fn arguments(pipe: &str, runtime: RuntimeInstanceId) -> Vec<String> {
    vec![
        "aura-core.exe".to_owned(),
        format!("--pipe-name={pipe}"),
        "--bootstrap-stdin".to_owned(),
        "--parent-pid=42".to_owned(),
        format!("--runtime-instance-id={runtime}"),
        "--component-manifest=C:\\Aura\\component-manifest.json".to_owned(),
    ]
}

fn secure_pipe(runtime: RuntimeInstanceId) -> String {
    format!(
        r"\\.\pipe\Aura\a47182cd8b9177c29a6be19b0c9c32a4\{runtime}\00112233445566778899aabbccddeeff"
    )
}

#[test]
fn parses_and_validates_a_secure_pipe_name() {
    let runtime = RuntimeInstanceId::new();
    let mut given = arguments(&secure_pipe(runtime), runtime);
    given.extend([
        "--data-root=C:\\Users\\u\\AppData\\Local\\Aura\\namespaces\\h".to_owned(),
        "--allow-dev-component-mismatch".to_owned(),
        "--test-mode=incompatible".to_owned(),
    ]);
    let options = CoreOptions::parse(given).expect("valid Core options");
    assert_eq!(options.test_mode, CoreTestMode::Incompatible);
    assert!(options.allow_dev_component_mismatch);
    assert_eq!(
        options.data_root,
        PathBuf::from("C:\\Users\\u\\AppData\\Local\\Aura\\namespaces\\h")
    );
}

#[test]
fn rejects_pipe_names_without_128_bits_of_randomness() {
    let runtime = RuntimeInstanceId::new();
    let mut given = arguments(
        &format!(r"\\.\pipe\Aura\a47182cd8b9177c29a6be19b0c9c32a4\{runtime}\short"),
        runtime,
    );
    given.push("--data-root=C:\\Aura".to_owned());
    assert!(CoreOptions::parse(given).is_err());
}

#[test]
fn core_is_not_started_without_the_data_folder_main_was_given() {
    let runtime = RuntimeInstanceId::new();
    let error = CoreOptions::parse(arguments(&secure_pipe(runtime), runtime))
        .expect_err("a Core without --data-root");
    assert!(error.to_string().contains("--data-root"), "{error}");
    let mut relative = arguments(&secure_pipe(runtime), runtime);
    relative.push("--data-root=Aura\\data".to_owned());
    assert!(CoreOptions::parse(relative).is_err());
}
