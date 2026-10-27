//! Crash exactly this fixture owner; all recorded Jobs and paths belong to the experiment.
use anyhow::{Result, ensure};
use aura_backend_windows_user::compat::CompatBackend;
use aura_windows_user_contract::{ManagedShell, ManagedShellCommand};
use std::{
    ffi::OsString,
    fs,
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

pub(super) fn run(root: &Path, args: &[OsString]) -> Result<()> {
    ensure!(args.len() == 2, "fixture managed payload/manifest required");
    let backend = CompatBackend::new(Path::new(&args[0]), Path::new(&args[1]))?;
    let fixture = std::env::current_exe()?;
    let command = ManagedShellCommand {
        shell: ManagedShell::Cmd,
        script: format!("\"{}\" tree \"{}\"", fixture.display(), root.display()),
        working_directory: root.display().to_string(),
        stdin: String::new(),
        timeout_ms: 15000,
        max_output_bytes: 4096,
        secret_environment: Vec::new(),
    };
    let mut execution = backend.launch(&command, None)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while !root.join("tree.ready").exists() {
        ensure!(Instant::now() < deadline, "owner tree startup timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
    fs::write(root.join("release"), b"release")?;
    let direct = execution.run_direct(&AtomicBool::new(false))?;
    ensure!(direct.active_processes > 0, "real retained descendant required");
    let receipt = execution.receipt();
    let grandchild: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("grandchild.json"))?)?;
    fs::write(
        root.join("owner.ready"),
        serde_json::to_vec(
            &serde_json::json!({"installation":receipt.installation,"nonce":receipt.nonce,"root_pid":receipt.root.process_id,"root_created":receipt.root.creation_time_100ns,"grandchild":grandchild}),
        )?,
    )?;
    std::thread::sleep(Duration::from_secs(30));
    Ok(())
}
