//! Owned local process fixtures, never a product entry point.
mod owner;
use anyhow::{Result, ensure};
use std::{
    fs,
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn heartbeat(root: &Path) -> Result<()> {
    let identity = aura_windows_platform::snapshot_process(std::process::id())?;
    fs::write(
        root.join("grandchild.json"),
        serde_json::to_vec(
            &serde_json::json!({"pid":identity.process_id,"created":identity.creation_time_100ns}),
        )?,
    )?;
    let mut output =
        fs::OpenOptions::new().create(true).append(true).open(root.join("heartbeat"))?;
    let end = Instant::now() + Duration::from_secs(30);
    while Instant::now() < end {
        output.write_all(b".")?;
        output.flush()?;
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn child(root: &Path, flags: u32) -> std::io::Result<std::process::Child> {
    Command::new(std::env::current_exe()?)
        .arg("heartbeat")
        .arg(root)
        .stdin(Stdio::null())
        .creation_flags(flags)
        .spawn()
}

fn await_release(root: &Path) -> Result<()> {
    let end = Instant::now() + Duration::from_secs(20);
    while !root.join("release").exists() {
        ensure!(Instant::now() < end, "fixture release timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}

fn main() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    let mode =
        args.first().and_then(|v| v.to_str()).ok_or_else(|| anyhow::anyhow!("fixture mode"))?;
    let root = PathBuf::from(args.get(1).ok_or_else(|| anyhow::anyhow!("owned fixture root"))?);
    ensure!(root.is_absolute() && root.is_dir(), "owned absolute directory required");
    match mode {
        "heartbeat" => heartbeat(&root)?,
        "tree" => {
            fs::write(root.join("starts"), b"1")?;
            let mut grandchild = child(&root, 0x0800_0000)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            while !root.join("heartbeat").exists() {
                if Instant::now() >= deadline {
                    let _ = grandchild.kill();
                    let _ = grandchild.wait();
                    anyhow::bail!("grandchild heartbeat startup timeout");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            fs::write(root.join("tree.ready"), b"ready")?;
            if let Err(error) = await_release(&root) {
                let _ = grandchild.kill();
                let _ = grandchild.wait();
                return Err(error);
            }
        }
        "breakaway" => {
            // CREATE_BREAKAWAY_FROM_JOB; on unexpected success reclaim this exact child.
            let refused = match child(&root, 0x0100_0000 | 0x0800_0000) {
                Ok(mut value) => {
                    let _ = value.kill();
                    let _ = value.wait();
                    false
                }
                Err(error) => error.raw_os_error() == Some(5),
            };
            fs::write(
                root.join("flags.json"),
                serde_json::to_vec(&serde_json::json!({"breakaway_refused":refused}))?,
            )?;
            await_release(&root)?;
        }
        "detached" => {
            // DETACHED_PROCESS + CREATE_NEW_PROCESS_GROUP, neither means breakaway.
            let mut grandchild = child(&root, 0x0000_0008 | 0x0000_0200)?;
            fs::write(root.join("flags.json"), b"{}")?;
            if let Err(error) = await_release(&root) {
                let _ = grandchild.kill();
                let _ = grandchild.wait();
                return Err(error);
            }
        }
        "nested" => {
            let cmd = aura_windows_platform::windows_directory()?.join("System32/cmd.exe");
            let arguments =
                ["/d", "/c", "set /p AURA_FIXTURE_WAIT="].map(|v| v.encode_utf16().collect());
            let process = aura_windows_platform::spawn_standard_user_process(
                &cmd,
                &arguments,
                &root,
                &[],
                None,
            )?;
            ensure!(process.active_processes_in_job()? > 0, "nested job has actual child");
            fs::write(root.join("nested.ready"), b"ready")?;
            let released = await_release(&root);
            process.terminate(1)?;
            ensure!(
                process.wait_for_process_tree_exit(Duration::from_secs(3))?,
                "nested stop unconfirmed"
            );
            released?;
        }
        "owner" => owner::run(&root, &args[2..])?,
        _ => anyhow::bail!("unknown fixture mode"),
    }
    Ok(())
}
