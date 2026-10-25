use super::*;
use std::{os::windows::ffi::OsStrExt, path::PathBuf, time::Duration};

fn value_of(
    entries: &[aura_windows_platform::NativeEnvironmentEntry],
    name: &str,
) -> Option<String> {
    entries
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(name))
        .map(|entry| String::from_utf16_lossy(&entry.value_utf16))
}

#[test]
fn the_execution_baseline_carries_what_creating_an_appcontainer_child_needs() {
    // `LOCALAPPDATA` is not decoration here. Creating an AppContainer child resolves
    // the container's local storage through it *in the environment block the launch
    // is handed*, and this baseline replaces the parent environment; a plan without
    // it fails with ERROR_ENVVAR_NOT_FOUND (203) before the child image is
    // considered. A fixture that omitted it produced exactly that failure and looked
    // like a platform defect, so the production baseline is pinned here.
    let directory = tempfile::tempdir().expect("test directory");
    let work = directory.path().join("work");
    std::fs::create_dir(&work).expect("work directory");
    let entries = baseline(&work, None).expect("baseline");
    let local_app_data = value_of(&entries, "LOCALAPPDATA").expect("LOCALAPPDATA is required");
    assert_eq!(
        PathBuf::from(&local_app_data),
        aura_windows_platform::dos_drive_path(&work).expect("dospath"),
        "LOCALAPPDATA must be Aura's own work directory, never the launching user's"
    );
    for name in ["SystemRoot", "WINDIR", "PATH", "PATHEXT", "TEMP", "TMP", "USERPROFILE"] {
        assert!(value_of(&entries, name).is_some(), "missing {name}");
    }
}

#[test]
#[ignore = "requires the locally cached pinned PowerShell package; no runtime download"]
fn managed_powershell_bound_image_initializes_file_provider() {
    check_file_provider(false);
}

#[test]
#[ignore = "requires an existing prepared execution installation; never installs resources"]
fn managed_powershell_restricted_token_initializes_file_provider() {
    check_file_provider(true);
}

fn check_file_provider(restricted: bool) {
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64/payload");
    assert!(runtime.join("pwsh.exe").is_file(), "pinned PowerShell package required");
    let directory = tempfile::tempdir().expect("test directory");
    let workspace = directory.path().join("workspace");
    let scratch = directory.path().join("scratch");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::create_dir(&scratch).expect("scratch");
    let write_sid = "S-1-5-21-39012617-41782562-31918343-7001";
    if restricted {
        aura_windows_platform::grant_empty_execution_write_root(&workspace, write_sid)
            .expect("temporary workspace grant");
        aura_windows_platform::grant_empty_execution_write_root(&scratch, write_sid)
            .expect("temporary scratch grant");
    }
    std::fs::write(workspace.join("input.txt"), "authorized input").expect("input");
    let script = scratch.join("command.ps1");
    std::fs::write(
        &script,
        format!(
            r#"{POWERSHELL_BOOTSTRAP}
[Console]::WriteLine(('HOME=' + $PSHOME + ';PWD=' + $PWD))
$read = [IO.File]::ReadAllText((Join-Path $PWD 'input.txt'))
if ($read -ne 'authorized input') {{ throw 'wrong input' }}
[Console]::WriteLine(('LOCATION=' + (Get-Location).Path))
'provider write' | Set-Content -LiteralPath (Join-Path $PWD 'written.txt')
[Console]::WriteLine('FILE-PROVIDER-OK')
"#
        ),
    )
    .expect("script");
    let image = aura_windows_platform::inspect_existing_path(&runtime.join("pwsh.exe"), false)
        .expect("image binding");
    let cwd = aura_windows_platform::inspect_existing_path(&workspace, false).expect("cwd binding");
    let _root_lease = aura_windows_platform::ExecutionFileSecurity::open(&workspace, true)
        .expect("retained workspace lease");
    let file =
        aura_windows_platform::inspect_existing_path(&script, false).expect("script binding");
    let args = POWERSHELL_ARGUMENTS
        .into_iter()
        .chain(["-ExecutionPolicy", "Bypass", "-File"])
        .map(|v| v.encode_utf16().collect())
        .chain([file.final_path.as_os_str().encode_wide().collect()])
        .collect::<Vec<_>>();
    let installation = restricted.then(|| {
        std::env::var("AURA_TEST_EXISTING_INSTALLATION")
            .expect("explicit existing installation required")
    });
    let restricted_process = restricted.then(|| {
        aura_windows_platform::spawn_restricted_user_process(
            write_sid,
            &image.final_path,
            &args,
            &cwd.final_path,
            &baseline(&scratch, Some(&runtime)).expect("production environment"),
            None,
            aura_windows_platform::RestrictedUserLaunchOptions {
                device_install_id: installation.as_deref(),
                cmd_script: None,
            },
        )
        .expect("restricted token and private desktop")
    });
    let standard_process = (!restricted).then(|| {
        aura_windows_platform::spawn_standard_user_process(
            &image.final_path,
            &args,
            &cwd.final_path,
            &baseline(&scratch, Some(&runtime)).expect("production environment"),
            None,
        )
        .expect("spawn")
    });
    let process = restricted_process.as_deref().or(standard_process.as_ref()).expect("process");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit = None;
    for _ in 0..1000 {
        stdout.extend(process.drain_stdout(65536).expect("stdout"));
        stderr.extend(process.drain_stderr(65536).expect("stderr"));
        exit = process.wait_timeout(Duration::from_millis(10)).expect("wait");
        if exit.is_some() {
            break;
        }
    }
    stdout.extend(process.drain_stdout(65536).expect("stdout tail"));
    stderr.extend(process.drain_stderr(65536).expect("stderr tail"));
    let stdout = String::from_utf8_lossy(&stdout);
    let stderr = String::from_utf8_lossy(&stderr);
    assert_eq!(exit, Some(0), "stdout={stdout}\nstderr={stderr}");
    assert!(stdout.contains("FILE-PROVIDER-OK"), "{stdout}\n{stderr}");
    assert_eq!(
        std::fs::read_to_string(workspace.join("written.txt")).expect("written"),
        "provider write\r\n"
    );
}
