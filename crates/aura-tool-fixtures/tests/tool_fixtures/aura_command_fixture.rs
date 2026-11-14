use std::process::Command;

#[test]
fn command_fixture_has_a_deterministic_success_result() {
    let output = Command::new(env!("CARGO_BIN_EXE_aura-command-fixture"))
        .output()
        .expect("run source-built command fixture");

    assert!(output.status.success());
    assert_eq!(output.stdout, b"authorized command fixture\n");
    assert!(output.stderr.is_empty());
}
