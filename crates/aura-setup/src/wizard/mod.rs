//! The setup program's and the uninstaller's windows: one window whose pages follow the work,
//! drawn by `aura-pixel-window`. Quiet runs never come here.
mod choose;
pub(crate) mod finish;
pub mod pages;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use anyhow::Result;
use aura_launcher::read_install_state;
use aura_pixel_window::{Action, Page, PixelWindow, Progress, choose_folder};

use crate::{
    data_choice::{self, DataChoice},
    host::Host,
    install::{self, AuraIsRunning, Kind, Options},
    payload, relocate, uninstall,
};

/// What the windows do beyond the installation itself, so tests can stand in for it.
pub trait Outside: Sync {
    fn show_licenses(&self, program: &Path);
    /// Starts the installed Aura.
    fn start(&self, root: &Path);
    fn choose_folder(&self, window: &PixelWindow, title: &str) -> Option<PathBuf>;
}

/// The real ones: Notepad, the installed launcher, the system's folder picker.
pub struct System;

impl Outside for System {
    /// The license texts the release carries, written beside each other in a temporary folder
    /// and opened in Notepad.
    fn show_licenses(&self, program: &Path) {
        let Ok(mut archive) = payload::open_in_program(program) else { return };
        let Ok(version) = payload::version_of(&mut archive) else { return };
        let folder = env::temp_dir().join("Aura Setup licenses");
        let _ = fs::create_dir_all(&folder);
        for name in ["LICENSE.txt", "THIRD-PARTY-NOTICES.txt"] {
            if let Ok(Some(bytes)) =
                payload::read_entry(&mut archive, &format!("versions/{version}/{name}"))
            {
                let path = folder.join(name);
                if fs::write(&path, bytes).is_ok() {
                    let _ = Command::new("notepad.exe").arg(&path).spawn();
                }
            }
        }
    }

    fn start(&self, root: &Path) {
        let _ = Command::new(root.join(payload::LAUNCHER)).current_dir(root).spawn();
    }

    fn choose_folder(&self, window: &PixelWindow, title: &str) -> Option<PathBuf> {
        choose_folder(Some(window.handle()), title).ok().flatten()
    }
}

/// Shows a page that only needs to be read, until it is closed.
fn tell(window: &mut PixelWindow, mut page: Page) -> Result<()> {
    window.show(&mut page)?;
    Ok(())
}

/// A window of its own that says why nothing can be done.
fn refuse(version: &str, heading: &str, error: &anyhow::Error) -> Result<()> {
    let page = pages::message(version, heading, &format!("{error:#}"));
    let mut window = PixelWindow::open(&page)?;
    tell(&mut window, page)
}

/// Waits while Aura runs from `root`; `false` when the user gives up.
fn wait_until_closed(
    window: &mut PixelWindow,
    host: &dyn Host,
    root: &Path,
    page: &Page,
) -> Result<bool> {
    loop {
        match host.running_in(root) {
            Ok(running) if running.is_empty() => return Ok(true),
            Ok(_) => {}
            Err(_) => return Ok(true),
        }
        let mut asked = page.clone();
        if window.show(&mut asked)? != Action::Button("retry") {
            return Ok(false);
        }
    }
}

/// Installs, upgrades or repairs from the release `program` carries.
pub fn install(host: &(dyn Host + Sync), program: &Path, outside: &dyn Outside) -> Result<()> {
    let opened = payload::open_in_program(program).and_then(|mut archive| {
        let release = install::read_release(&mut archive)?;
        Ok((archive, release))
    });
    let (mut archive, release) = match opened {
        Ok(opened) => opened,
        Err(error) => return refuse("", "无法安装", &error),
    };
    let version = release.version.clone();
    let plan = match install::plan(host, &release, None) {
        Ok(plan) => plan,
        Err(error) => return refuse(&version, "无法安装", &error),
    };
    let licenses = payload::read_entry(&mut archive, &format!("versions/{version}/LICENSE.txt"))
        .is_ok_and(|entry| entry.is_some());
    let chosen =
        match choose::choose(host, outside, program, Some(&release), plan, &version, licenses) {
            Ok(Some(chosen)) => chosen,
            Ok(None) => return Ok(()),
            Err(error) => return refuse(&version, "无法安装", &error),
        };
    let (mut window, chosen) = chosen;
    let plan = chosen.plan.clone();
    let shortcut = chosen.shortcut;
    if !wait_until_closed(
        &mut window,
        host,
        &plan.root,
        &pages::running(pages::running_frame(&plan.kind, &version)),
    )? {
        return Ok(());
    }
    let options =
        Options { desktop_shortcut: plan.kind == Kind::Fresh && shortcut, while_running: false };
    let progress = Progress::default();
    let working = pages::working(&plan.kind, &version, &progress.snapshot());
    let result = window.run_work(
        &working,
        |page| *page = pages::working(&plan.kind, &version, &progress.snapshot()),
        || {
            install::install(
                host,
                program,
                &mut archive,
                &release,
                &plan,
                &options,
                &mut |done, total| {
                    progress.set_total(total);
                    progress.set_done(done);
                },
            )
        },
    );
    match result {
        Ok(()) => {
            let notes = match carry_out(&mut window, host, &plan.kind, &version, &chosen) {
                Ok(notes) => notes,
                Err(error) => {
                    return tell(
                        &mut window,
                        pages::failed(&plan.kind, &version, &format!("{error:#}"), None),
                    );
                }
            };
            finish::finish(&mut window, host, outside, (&plan.kind, &version), notes, chosen.root())
        }
        Err(error) if error.is::<AuraIsRunning>() => tell(
            &mut window,
            pages::failed(
                &plan.kind,
                &version,
                "Aura 在安装时启动了。请关闭 Aura 后重新运行安装程序。",
                None,
            ),
        ),
        Err(error) => tell(
            &mut window,
            pages::failed(
                &plan.kind,
                &version,
                &format!("{error:#}"),
                Some("已安装的 Aura 没有改变。"),
            ),
        ),
    }
}

/// What follows the program's own installation: moving the installation, then the data choice.
/// A move that fails stops here with Aura where it was; a data choice that cannot be carried out
/// leaves the data where it is and becomes a note.
fn carry_out(
    window: &mut PixelWindow,
    host: &(dyn Host + Sync),
    kind: &Kind,
    version: &str,
    chosen: &choose::Chosen,
) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    if let Some(relocation) = &chosen.move_to {
        let progress = Progress::default();
        let stage = AtomicUsize::new(0);
        let moved = window.run_work(
            &pages::moving(kind, version, 0, &progress.snapshot()),
            |page| {
                *page = pages::moving(
                    kind,
                    version,
                    stage.load(Ordering::Acquire),
                    &progress.snapshot(),
                );
            },
            || {
                relocate::relocate(
                    host,
                    relocation,
                    &mut |at, done, total| {
                        let at = match at {
                            relocate::Stage::Copying => 0,
                            relocate::Stage::Verifying => 1,
                            relocate::Stage::Registering => 2,
                            relocate::Stage::Removing => 3,
                        };
                        stage.store(at, Ordering::Release);
                        progress.set_total(total);
                        progress.set_done(done);
                    },
                    &|| false,
                    None,
                )
            },
        )?;
        notes.extend(moved.notes);
        if !moved.left_behind.is_empty() {
            notes.push(format!("{} 里还有不是 Aura 的文件，文件夹留着", relocation.from.display()));
        }
    }
    let namespace = read_install_state(chosen.root())?.release_descriptor.bundle_namespace;
    match data_choice::apply(host, chosen.root(), &namespace, &chosen.data) {
        Ok(()) => match &chosen.data {
            DataChoice::Move(to) => {
                notes.push(format!("下次启动 Aura 时，数据搬到 {}", to.display()))
            }
            DataChoice::Reuse(folder) => notes.push(format!("使用 {} 里的数据", folder.display())),
            DataChoice::Start(folder) => notes.push(format!("数据放在 {}", folder.display())),
            DataChoice::Keep => {}
        },
        Err(error) => notes.push(format!("数据位置没有改：{error:#}")),
    }
    Ok(notes)
}

/// Moves the installation in `root` or its data, from the installed program. `from_aura`: Aura
/// closed itself to open this, so giving up starts it again.
pub fn relocate(
    host: &(dyn Host + Sync),
    root: &Path,
    outside: &dyn Outside,
    from_aura: bool,
) -> Result<()> {
    let version = match read_install_state(root) {
        Ok(state) => state.active,
        Err(error) => return refuse("", "无法移动", &error),
    };
    let plan = install::Plan { root: root.to_path_buf(), kind: Kind::Relocate };
    let program = env::current_exe()?;
    let (mut window, chosen) =
        match choose::choose(host, outside, &program, None, plan, &version, false) {
            Ok(Some(chosen)) => chosen,
            Ok(None) => {
                if from_aura {
                    outside.start(root);
                }
                return Ok(());
            }
            Err(error) => return refuse(&version, "无法移动", &error),
        };
    if !wait_until_closed(
        &mut window,
        host,
        root,
        &pages::running(pages::running_frame(&Kind::Relocate, &version)),
    )? {
        if from_aura {
            outside.start(root);
        }
        return Ok(());
    }
    match carry_out(&mut window, host, &Kind::Relocate, &version, &chosen) {
        Ok(notes) => finish::finish(
            &mut window,
            host,
            outside,
            (&Kind::Relocate, &version),
            notes,
            chosen.root(),
        ),
        Err(error) => {
            tell(
                &mut window,
                pages::failed(&Kind::Relocate, &version, &format!("{error:#}"), None),
            )?;
            if from_aura {
                outside.start(root);
            }
            Ok(())
        }
    }
}

/// Uninstalls the installation in `root`, with its data when the user ticks the box.
pub fn uninstall(host: &(dyn Host + Sync), root: &Path) -> Result<()> {
    let plan = match uninstall::plan_uninstall(host, root) {
        Ok(plan) => plan,
        Err(error) => return refuse("", "无法卸载", &error),
    };
    let mut delete_data = false;
    let mut page = pages::confirm_uninstall(&plan, delete_data);
    let mut window = PixelWindow::open(&page)?;
    loop {
        let action = window.show(&mut page)?;
        delete_data = page.checked("delete-data");
        match action {
            Action::Toggled(_) => page = pages::confirm_uninstall(&plan, delete_data),
            Action::Button("remove") => break,
            _ => return Ok(()),
        }
    }
    if !wait_until_closed(&mut window, host, root, &pages::uninstall_running(&plan))? {
        return Ok(());
    }
    let result = window.run_work(
        &pages::uninstalling(&plan),
        |_| {},
        || uninstall::uninstall(host, &plan, delete_data),
    );
    match result {
        Ok(_) => tell(&mut window, pages::uninstalled(&plan, delete_data)),
        Err(error) => tell(&mut window, pages::uninstall_failed(&plan, &format!("{error:#}"))),
    }
}
