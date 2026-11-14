use super::support::*;
use aura_backend_windows_user::compat::CompatBackend;
use aura_windows_user_contract::ManagedShell;
use std::{ffi::OsString, path::PathBuf, sync::atomic::AtomicBool};

#[test]
fn empty_path_still_uses_pinned_roots_and_user_shims_use_the_selected_path() {
    let directory = tempfile::tempdir().expect("owned PATH fixture");
    let runtime = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    let shim = directory.path().join("shim with 空格");
    std::fs::create_dir(&shim).expect("owned shim directory");
    std::fs::write(shim.join("aura_gate_shim.cmd"), "@echo shim-selected>shim-proof.txt\r\n")
        .expect("local user tool shim");
    std::fs::write(shim.join("pwsh.cmd"), "@echo WRONG-ROOT>wrong-root.txt\r\n")
        .expect("local shadow interpreter");
    for path in [OsString::new(), shim.as_os_str().to_owned()] {
        // A process started from cmd or PowerShell also carries Windows' hidden `=C:`-style
        // entries; they are not variables.
        let mut environment = std::env::vars_os()
            .filter(|(key, _)| !key.to_string_lossy().starts_with('='))
            .filter(|(key, _)| !key.to_string_lossy().eq_ignore_ascii_case("PATH"))
            .collect::<Vec<_>>();
        environment.push(("PATH".into(), path.clone()));
        let backend = CompatBackend::with_environment(
            &runtime.join("payload"),
            &runtime.join("manifest.json"),
            environment,
        )
        .expect("verified roots with selected fixture PATH");
        for shell in [ManagedShell::PowerShell, ManagedShell::Cmd] {
            let script = if path.is_empty() {
                match shell {
                    ManagedShell::PowerShell => "[Console]::WriteLine('pinned root')",
                    ManagedShell::Cmd => "echo pinned root",
                }
            } else {
                match shell {
                    ManagedShell::PowerShell => "& aura_gate_shim.cmd",
                    ManagedShell::Cmd => "call aura_gate_shim.cmd",
                }
            };
            let mut execution = backend
                .launch(&command(shell, directory.path(), script), None)
                .expect("launch pinned interpreter");
            let result = execution.run_direct(&AtomicBool::new(false)).expect("actual PATH result");
            assert_eq!(result.exit_code, Some(0), "{result:?}");
            assert!(!directory.path().join("wrong-root.txt").exists());
            if !path.is_empty() {
                assert_eq!(
                    std::fs::read_to_string(directory.path().join("shim-proof.txt"))
                        .expect("selected shim artifact")
                        .trim(),
                    "shim-selected"
                );
                std::fs::remove_file(directory.path().join("shim-proof.txt"))
                    .expect("clear only owned shim artifact between shells");
            }
        }
    }
}
