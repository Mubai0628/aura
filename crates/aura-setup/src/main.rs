#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![forbid(unsafe_code)]
//! The Aura setup program, and the installed uninstaller (the same program without the release).
//!
//!   Aura-Setup.exe                                       install, upgrade or repair, with windows
//!   Aura-Setup.exe --quiet [--root <dir>] [--data <dir>] [--desktop-shortcut]
//!   Aura-Setup.exe --quiet [--move-to <dir>] [--data <dir>]   an existing installation
//!   Uninstall Aura.exe --uninstall [--quiet [--delete-data]]
//!   Uninstall Aura.exe --relocate [--from-aura]     move the installation or the data, with windows
//!   Uninstall Aura.exe --quiet [--move-to <dir>] [--data <dir>]
//!   Uninstall Aura.exe --apply-update <package>     a newer version waits as a trial
//!   Uninstall Aura.exe --roll-back                  the previous version waits as a trial
//!
//! `--data` is where the data goes: at a new installation a folder holding its data is used as it
//! is; otherwise the data moves there at the next start. `--move-to` moves the installation.
//! Quiet runs print one JSON line and exit 0 on success, 2 while Aura is running, 1 otherwise.
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
    data_choice::{self, DataChoice},
    folder_check,
    host::{Host, SystemHost},
    install::{self, AuraIsRunning, Options},
    payload, relocate, uninstall, wizard,
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
    data: Option<PathBuf>,
    move_to: Option<PathBuf>,
    relocate: bool,
    from_aura: bool,
    relocate_from: Option<PathBuf>,
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
        data: None,
        move_to: None,
        relocate: false,
        from_aura: false,
        relocate_from: None,
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
            Some("--data") => arguments.data = items.next().map(PathBuf::from),
            Some("--move-to") => arguments.move_to = items.next().map(PathBuf::from),
            Some("--relocate") => arguments.relocate = true,
            Some("--from-aura") => arguments.from_aura = true,
            Some("--relocate-from") => arguments.relocate_from = items.next().map(PathBuf::from),
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
    let fresh = plan.kind == install::Kind::Fresh;
    anyhow::ensure!(
        !(fresh && arguments.move_to.is_some()),
        "--move-to moves an existing installation; a new one takes --root"
    );
    let namespace = release.manifest.bundle_namespace.clone();
    let on_trial = !fresh && plan.kind != install::Kind::Repair;
    let state = (!fresh).then(|| aura_launcher::read_install_state(&plan.root)).transpose()?;
    let relocation = match &arguments.move_to {
        Some(to) => Some(
            relocate::plan(&SystemHost, &plan.root, to)
                .map_err(|refusals| anyhow::anyhow!(folder_check::joined(&refusals)))?,
        ),
        None => None,
    };
    let root = relocation.as_ref().map_or(plan.root.clone(), |relocation| relocation.to.clone());
    let data = quiet_data_choice(arguments, &namespace, state.as_ref(), &root, fresh, on_trial)?;
    install::install(
        &SystemHost,
        program,
        &mut archive,
        &release,
        &plan,
        &Options { desktop_shortcut: arguments.desktop_shortcut, while_running: false },
        &mut |_, _| {},
    )?;
    let notes = quiet_moves(relocation.as_ref(), &root, &namespace, &data)?;
    Ok(json!({
        "installed": release.version,
        "root": root,
        "kind": format!("{:?}", plan.kind),
        "signature": format!("{:?}", release.signature),
        "data": format!("{data:?}"),
        "notes": notes,
    }))
}

/// `--data` checked before anything changes.
fn quiet_data_choice(
    arguments: &Arguments,
    namespace: &aura_runtime_contract::BundleNamespace,
    state: Option<&aura_runtime_contract::ActiveInstallState>,
    root: &Path,
    fresh: bool,
    on_trial: bool,
) -> Result<DataChoice> {
    let Some(data) = &arguments.data else { return Ok(DataChoice::Keep) };
    let locations = data_choice::locations(&SystemHost, namespace, state)?;
    data_choice::choose(&SystemHost, namespace, &locations, data, root, fresh, on_trial)
        .map_err(|reasons| anyhow::anyhow!("不能放在 {}：{}", data.display(), reasons.join("；")))
}

/// The move of the installation, then the data choice; the notes a window's last page would show.
fn quiet_moves(
    relocation: Option<&relocate::Relocation>,
    root: &Path,
    namespace: &aura_runtime_contract::BundleNamespace,
    data: &DataChoice,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    if let Some(relocation) = relocation {
        let moved =
            relocate::relocate(&SystemHost, relocation, &mut |_, _, _| {}, &|| false, None)?;
        notes.extend(moved.notes);
        notes.extend(moved.left_behind.iter().map(|path| format!("left {}", path.display())));
    }
    data_choice::apply(&SystemHost, root, namespace, data)?;
    Ok(notes)
}

/// Moving the installation or its data from the installed program, which runs from a copy in
/// the temporary folder: the installation's folder is what moves.
fn quiet_relocate(root: &Path, arguments: &Arguments) -> Result<serde_json::Value> {
    let state = aura_launcher::read_install_state(root)?;
    let namespace = state.release_descriptor.bundle_namespace.clone();
    let relocation = match &arguments.move_to {
        Some(to) => Some(
            relocate::plan(&SystemHost, root, to)
                .map_err(|refusals| anyhow::anyhow!(folder_check::joined(&refusals)))?,
        ),
        None => None,
    };
    let target = relocation.as_ref().map_or(root.to_path_buf(), |relocation| relocation.to.clone());
    let data = quiet_data_choice(
        arguments,
        &namespace,
        Some(&state),
        &target,
        false,
        state.pending.is_some(),
    )?;
    let notes = quiet_moves(relocation.as_ref(), &target, &namespace, &data)?;
    Ok(json!({ "root": target, "data": format!("{data:?}"), "notes": notes }))
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

/// Moving the installation moves the running program's folder, so the installed program continues
/// from a copy of itself in the temporary folder, as uninstalling does.
fn hand_over_relocation(program: &Path, arguments: &Arguments) -> Result<()> {
    let root = program.parent().context("the installed program has no folder")?;
    let copy = env::temp_dir().join(format!("aura-relocate-{}.exe", std::process::id()));
    fs::copy(program, &copy)?;
    let mut command = Command::new(&copy);
    command.arg("--relocate-from").arg(root).arg("--wait").arg(std::process::id().to_string());
    if arguments.quiet {
        command.arg("--quiet");
    }
    if arguments.from_aura {
        command.arg("--from-aura");
    }
    if let Some(to) = &arguments.move_to {
        command.arg("--move-to").arg(to);
    }
    if let Some(data) = &arguments.data {
        command.arg("--data").arg(data);
    }
    // As with uninstalling, a quiet copy answers on the output it inherits.
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
                // A folder another program holds says nothing against the package: the caller keeps
                // it and installs it again once the folder is free.
                let intact = error.is::<install::FolderInUse>();
                report(json!({
                    "error": format!("{error:#}"),
                    "package_intact": intact,
                    "code": install::refusal_code(&error),
                }));
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
            wizard::uninstall(&SystemHost, root)?;
            ExitCode::SUCCESS
        };
        remove_self(&program);
        return Ok(code);
    }
    if let Some(root) = &arguments.relocate_from {
        if let Some(process_id) = arguments.wait_for {
            wait_for_exit(root, process_id);
        }
        let code = if arguments.quiet {
            match quiet_relocate(root, &arguments) {
                Ok(value) => {
                    report(value);
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    report(json!({ "error": format!("{error:#}") }));
                    exit_for(&error)
                }
            }
        } else {
            wizard::relocate(&SystemHost, root, &wizard::System, arguments.from_aura)?;
            ExitCode::SUCCESS
        };
        remove_self(&program);
        return Ok(code);
    }
    if arguments.uninstall {
        hand_over(&program, &arguments)?;
        return Ok(ExitCode::SUCCESS);
    }
    let carries_release = payload::open_in_program(&program).is_ok();
    if !carries_release
        && (arguments.relocate || arguments.move_to.is_some() || arguments.data.is_some())
    {
        hand_over_relocation(&program, &arguments)?;
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
    wizard::install(&SystemHost, &program, &wizard::System)?;
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
