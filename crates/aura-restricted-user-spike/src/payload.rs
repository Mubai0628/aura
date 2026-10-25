use anyhow::{Result, bail, ensure};
use aura_windows_platform::{capture_file_dacl, current_user_token_identity, restore_file_dacl};
use serde_json::json;
use std::{
    fs,
    io::{Read, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::Command,
    time::Duration,
};

pub fn run(kind: &str, root: &Path) -> Result<()> {
    ensure!(root.join("r06-owned-fixture").is_file(), "unowned fixture root");
    match kind {
        "runtime" => println!(
            "{}",
            json!({"dlls":aura_windows_platform::spike_runtime_dlls(),"devices":crate::compatibility::crypto_devices(),"ksec_access":aura_windows_platform::ExecutionDevice::Ksec.access_probe(),"synchronization":aura_windows_platform::spike_synchronization(),"namespace_access":aura_windows_platform::execution_namespace_access()})
        ),
        "loopback" => crate::network::payload(root)?,
        "access" => {
            let mut stdin = Vec::new();
            std::io::stdin().take(4096).read_to_end(&mut stdin)?;
            let file = root.join("allowed/write.txt");
            let write = fs::write(&file, b"allowed").is_ok();
            let moved = root.join("allowed/moved.txt");
            let rename = fs::rename(&file, &moved).is_ok();
            let delete = fs::remove_file(&moved).is_ok();
            let read = fs::read(root.join("denied/read.txt")).is_ok();
            let outside_denied = fs::write(root.join("denied/escape.txt"), b"forbidden").is_err();
            let hardlink_denied =
                fs::write(root.join("allowed/escape-link.txt"), b"forbidden").is_err();
            let current = capture_file_dacl(&root.join("denied"))?;
            // Even writing the same DACL requires WRITE_DAC. No broad ACE is
            // ever inserted by this negative test, including on a failed spike.
            let dacl_denied = restore_file_dacl(&root.join("denied"), &current).is_err();
            let child = Command::new(std::env::current_exe()?)
                .creation_flags(0x0800_0000)
                .args(["payload", "child"])
                .arg(root)
                .output()?;
            let child = serde_json::from_slice::<serde_json::Value>(&child.stdout)?;
            let (_, restricted) = current_user_token_identity()?;
            let secret_verified =
                std::env::var("AURA_R06_TRANSIENT").as_deref() == Ok("R06_ENV_CANARY");
            println!(
                "{}",
                json!({"write":write,"rename":rename,"delete":delete,"read":read,"outside_denied":outside_denied,"hardlink_denied":hardlink_denied,"dacl_denied":dacl_denied,"restricted":restricted,"stdin_verified":stdin==b"R06_STDIN_CANARY","secret_verified":secret_verified,"child":child,"null_access":crate::compatibility::null_access()})
            );
        }
        "child" => {
            let (_, restricted) = current_user_token_identity()?;
            println!(
                "{}",
                json!({"restricted":restricted,"allowed":fs::write(root.join("allowed/child.txt"),b"child").is_ok(),"outside_denied":fs::write(root.join("denied/child.txt"),b"forbidden").is_err()})
            );
        }
        "tree" => {
            let mut child = Command::new(std::env::current_exe()?)
                .creation_flags(0x0800_0000)
                .args(["payload", "wait"])
                .arg(root)
                .spawn()?;
            println!("child_ready");
            std::io::stdout().flush()?;
            let _ = child.wait()?;
        }
        "wait" => std::thread::sleep(Duration::from_secs(30)),
        "flood" => {
            let bytes = [b'x'; 4096];
            loop {
                std::io::stdout().write_all(&bytes)?;
                std::io::stderr().write_all(&bytes)?;
            }
        }
        _ => bail!("unknown fixed payload"),
    }
    Ok(())
}
