//! A credential reaches a command only when the command declares it, and no credential's value
//! reaches its output, through the managed PowerShell and its real pipes.
use super::support::command;
use aura_backend_windows_user::compat::CompatBackend;
use aura_windows_user_contract::ManagedShell;
use std::{ffi::OsString, path::PathBuf, sync::atomic::AtomicBool};

const DB: &str = "fixture-pass-7781";
const API: &str = "fixture-token-5521";

fn backend() -> CompatBackend {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    let mut environment = aura_windows_platform::current_user_environment().expect("environment");
    for (name, value) in [("ZZ_FIXTURE_DB_PASSWORD", DB), ("ZZ_FIXTURE_API_TOKEN", API)] {
        environment.push((OsString::from(name), OsString::from(value)));
    }
    CompatBackend::with_environment(&root.join("payload"), &root.join("manifest.json"), environment)
        .expect("verified local managed PS7 and system CMD candidate")
}

#[test]
fn a_declared_credential_is_usable_and_hidden_and_an_undeclared_one_is_absent_and_hidden() {
    let root = tempfile::tempdir().expect("owned compat workspace");
    let mut request = command(
        ManagedShell::PowerShell,
        root.path(),
        "[Console]::WriteLine(\"db=$env:ZZ_FIXTURE_DB_PASSWORD len=$($env:ZZ_FIXTURE_DB_PASSWORD.Length)\")
[Console]::WriteLine(\"api=[$env:ZZ_FIXTURE_API_TOKEN]\")
[Console]::WriteLine('copied=fixture-token-5521')
[Console]::Write('split=fixture-pa'); Start-Sleep -Milliseconds 300; [Console]::WriteLine('ss-7781')
[Console]::Error.WriteLine($env:ZZ_FIXTURE_DB_PASSWORD)",
    );
    request.secret_environment = vec!["ZZ_FIXTURE_DB_PASSWORD".to_owned()];
    let mut execution = backend().launch(&request, None).expect("launch in Job");
    let removed = execution.excluded_environment_names().to_vec();
    let result = execution.run_direct(&AtomicBool::new(false)).expect("actual direct terminal");
    let stdout = String::from_utf8(result.stdout).expect("UTF-8");
    let stderr = String::from_utf8(result.stderr).expect("UTF-8");
    assert!(stdout.contains("db=[已隐藏:ZZ_FIXTURE_DB_PASSWORD] len=17"), "{stdout}");
    assert!(stdout.contains("api=[]"), "the undeclared one is not passed: {stdout}");
    assert!(stdout.contains("copied=[已隐藏:ZZ_FIXTURE_API_TOKEN]"), "{stdout}");
    assert!(stdout.contains("split=[已隐藏:ZZ_FIXTURE_DB_PASSWORD]"), "{stdout}");
    assert!(stderr.contains("[已隐藏:ZZ_FIXTURE_DB_PASSWORD]"), "{stderr}");
    for text in [&stdout, &stderr] {
        assert!(!text.contains(DB) && !text.contains(API), "a value leaked: {text}");
    }
    assert!(removed.iter().any(|name| name == "ZZ_FIXTURE_API_TOKEN"), "{removed:?}");
    assert!(!removed.iter().any(|name| name == "ZZ_FIXTURE_DB_PASSWORD"), "{removed:?}");
}
