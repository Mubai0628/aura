#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
//! The Aura setup program, and the installed uninstaller (the same program without the release).
//!
//!   Aura-Setup.exe                                       install, upgrade or repair, with windows
//!   Aura-Setup.exe --quiet [--root <dir>] [--desktop-shortcut]
//!   Uninstall Aura.exe --uninstall [--quiet [--delete-data]]
//!   Uninstall Aura.exe --apply-update <package>     a newer version waits as a trial
//!   Uninstall Aura.exe --roll-back                  the previous version waits as a trial
//!
//! Quiet runs print one JSON line and exit 0 on success, 2 while Aura is running, 1 otherwise.
mod ui;

use std::{
    env, fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    thread,
    time::Duration,
};

use anyhow::{Context, Result};
use aura_setup::{
    host::{Host, SystemHost},
    install::{self, AuraIsRunning, Options},
    payload, uninstall,
};
use serde_json::json;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DETACHED_PROCESS: u32 = 0x0000_0008;

struct Arguments {
    quiet: bool,
    uninstall: bool,
    delete_data: bool,
    desktop_shortcut: bool,
    root: Option<PathBuf>,
    uninstall_from: Option<PathBuf>,
    wait_for: Option<u32>,
    apply_update: Option<PathBuf>,
    roll_back: bool,
}

fn parse() -> Result<Arguments> {
    let mut arguments = Arguments {
        quiet: false,
        uninstall: false,
        delete_data: false,
        desktop_shortcut: false,
        root: None,
        uninstall_from: None,
        wait_for: None,
        apply_update: None,
        roll_back: false,
    };
    let mut items = env::args_os().skip(1);
    while let Some(item) = items.next() {
        match item.to_str() {
            Some("--quiet") => arguments.quiet = true,
            Some("--uninstall") => arguments.uninstall = true,
            Some("--delete-data") => arguments.delete_data = true,
            Some("--desktop-shortcut") => arguments.desktop_shortcut = true,
            Some("--root") => arguments.root = items.next().map(PathBuf::from),
            Some("--uninstall-from") => arguments.uninstall_from = items.next().map(PathBuf::from),
            Some("--apply-update") => arguments.apply_update = items.next().map(PathBuf::from),
            Some("--roll-back") => arguments.roll_back = true,
            Some("--wait") => {
                arguments.wait_for = items.next().and_then(|value| value.to_str()?.parse().ok());
            }
            _ => anyhow::bail!("unknown argument: {}", item.to_string_lossy()),
        }
    }
    Ok(arguments)
}

fn report(value: serde_json::Value) {
    println!("{value}");
}

fn exit_for(error: &anyhow::Error) -> ExitCode {
    if error.is::<AuraIsRunning>() { ExitCode::from(2) } else { ExitCode::from(1) }
}

fn quiet_install(program: &Path, arguments: &Arguments) -> Result<serde_json::Value> {
    let mut archive = payload::open_in_program(program)?;
    let release = install::read_release(&mut archive)?;
    let plan = install::plan(&SystemHost, &release, arguments.root.clone())?;
    install::install(
        &SystemHost,
        program,
        &mut archive,
        &release,
        &plan,
        &Options { desktop_shortcut: arguments.desktop_shortcut, while_running: false },
        &mut |_, _| {},
    )?;
    Ok(json!({
        "installed": release.version,
        "root": plan.root,
        "kind": format!("{:?}", plan.kind),
        "signature": format!("{:?}", release.signature),
    }))
}

/// Uninstalling from inside the installation would remove the running program, so the first run
/// copies itself to the temporary folder and continues from there.
fn hand_over(program: &Path, arguments: &Arguments) -> Result<()> {
    let root = program.parent().context("the uninstaller has no folder")?;
    let copy = env::temp_dir().join(format!("aura-uninstall-{}.exe", std::process::id()));
    fs::copy(program, &copy)?;
    let mut command = Command::new(&copy);
    command.arg("--uninstall-from").arg(root).arg("--wait").arg(std::process::id().to_string());
    if arguments.quiet {
        command.arg("--quiet");
    }
    if arguments.delete_data {
        command.arg("--delete-data");
    }
    command.spawn()?;
    Ok(())
}

/// Removes this program a moment after it exits.
fn remove_self(program: &Path) {
    let _ = Command::new("cmd.exe")
        .args(["/d", "/c", "ping -n 3 127.0.0.1 >nul & del /f /q"])
        .arg(program)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn();
}

fn wait_for_exit(root: &Path, process_id: u32) {
    for _ in 0..100 {
        let still = SystemHost
            .running_in(root)
            .map(|running| running.iter().any(|program| program.process_id == process_id))
            .unwrap_or(false);
        if !still {
            return;
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn run() -> Result<ExitCode> {
    let arguments = parse()?;
    let program = env::current_exe()?;
    if arguments.apply_update.is_some() || arguments.roll_back {
        let root = program.parent().context("the installed setup program has no folder")?;
        let outcome = match &arguments.apply_update {
            Some(package) => install::apply_update(&SystemHost, &program, root, package)
                .map(|release| json!({ "waiting": release.version, "signature": format!("{:?}", release.signature) })),
            None => install::roll_back(root).map(|version| json!({ "waiting": version })),
        };
        return Ok(match outcome {
            Ok(value) => {
                report(value);
                ExitCode::SUCCESS
            }
            Err(error) => {
                report(json!({ "error": format!("{error:#}") }));
                exit_for(&error)
            }
        });
    }
    if let Some(root) = &arguments.uninstall_from {
        if let Some(process_id) = arguments.wait_for {
            wait_for_exit(root, process_id);
        }
        let code = if arguments.quiet {
            match uninstall::plan_uninstall(&SystemHost, root)
                .and_then(|plan| uninstall::uninstall(&SystemHost, &plan, arguments.delete_data))
            {
                Ok(removed) => {
                    report(
                        json!({ "uninstalled": root, "chrome": removed.chrome, "data": removed.data, "credentials": removed.credentials }),
                    );
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    report(json!({ "error": format!("{error:#}") }));
                    exit_for(&error)
                }
            }
        } else {
            ui::uninstall(root)?;
            ExitCode::SUCCESS
        };
        remove_self(&program);
        return Ok(code);
    }
    if arguments.uninstall {
        hand_over(&program, &arguments)?;
        return Ok(ExitCode::SUCCESS);
    }
    if arguments.quiet {
        return Ok(match quiet_install(&program, &arguments) {
            Ok(value) => {
                report(value);
                ExitCode::SUCCESS
            }
            Err(error) => {
                report(json!({ "error": format!("{error:#}") }));
                exit_for(&error)
            }
        });
    }
    ui::install(&program)?;
    Ok(ExitCode::SUCCESS)
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(1)
        }
    }
}
