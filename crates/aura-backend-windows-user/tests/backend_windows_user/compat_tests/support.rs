use aura_backend_windows_user::compat::CompatBackend;
use aura_windows_user_contract::{ManagedShell, ManagedShellCommand};
use std::{path::PathBuf, sync::atomic::AtomicBool};

pub(super) fn backend() -> CompatBackend {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    CompatBackend::new(&root.join("payload"), &root.join("manifest.json"))
        .expect("verified local managed PS7 and system CMD candidate")
}

pub(super) fn command(
    shell: ManagedShell,
    directory: &std::path::Path,
    script: &str,
) -> ManagedShellCommand {
    ManagedShellCommand {
        shell,
        script: script.into(),
        working_directory: directory.display().to_string(),
        stdin: String::new(),
        // Far above a PowerShell start on a busy machine, so a stalled start is not taken for the
        // script's own result. A test about timeouts sets its own.
        timeout_ms: 60_000,
        max_output_bytes: 4096,
        secret_environment: Vec::new(),
    }
}

pub(super) fn run(
    shell: ManagedShell,
    script: &str,
) -> aura_backend_windows_user::compat::DirectResult {
    let root = tempfile::tempdir().expect("owned compat workspace");
    let backend = backend();
    let mut execution =
        backend.launch(&command(shell, root.path(), script), None).expect("launch in Job");
    execution.run_direct(&AtomicBool::new(false)).expect("actual direct terminal")
}

/// What a failed assertion needs to tell a stopped run from the script's own result.
pub(super) fn describe(result: &aura_backend_windows_user::compat::DirectResult) -> String {
    format!(
        "stop={} confirmed={} exit={:?} elapsed={:?} stderr={}",
        result.stop_reason,
        result.stop_confirmed,
        result.exit_code,
        result.elapsed,
        String::from_utf8_lossy(&result.stderr)
    )
}

pub(super) fn fixture() -> &'static str {
    env!("CARGO_BIN_EXE_aura-compat-fixture")
}

pub(super) fn fixture_script(
    shell: ManagedShell,
    mode: &str,
    directory: &std::path::Path,
) -> String {
    match shell {
        ManagedShell::PowerShell => format!(
            "& '{}' {} '{}'",
            fixture().replace('\'', "''"),
            mode,
            directory.display().to_string().replace('\'', "''")
        ),
        ManagedShell::Cmd => format!("\"{}\" {} \"{}\"", fixture(), mode, directory.display()),
    }
}

pub(super) fn wait_file(path: &std::path::Path) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !path.exists() {
        assert!(std::time::Instant::now() < deadline, "fixture did not publish {}", path.display());
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
