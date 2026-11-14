//! The location page: where the program and the data go, each with 更改, until the user goes on
//! or gives up. A new installation's folder is planned again for the folder picked; an existing
//! one is checked for the move. A data folder is checked as the storage page checks one.
use std::path::{Path, PathBuf};

use anyhow::Result;
use aura_launcher::read_install_state;
use aura_pixel_window::{Action, Page, PixelWindow};

use super::{Outside, pages};
use crate::{
    data_choice::{self, DataChoice},
    folder_check::{self, root_in},
    host::Host,
    install::{self, Kind, Plan, Release},
    relocate::{self, Relocation},
};

/// What the user settled on.
pub(super) struct Chosen {
    pub plan: Plan,
    pub move_to: Option<Relocation>,
    pub data: DataChoice,
    pub shortcut: bool,
}

impl Chosen {
    /// Where the installation is once everything chosen is done.
    pub fn root(&self) -> &Path {
        self.move_to.as_ref().map_or(&self.plan.root, |relocation| &relocation.to)
    }
}

/// Shows the location page in a new window; `None` when the user gave up. `release` is the one a
/// setup program carries, `None` when the installed program only moves the installation.
pub(super) fn choose(
    host: &(dyn Host + Sync),
    outside: &dyn Outside,
    program: &Path,
    release: Option<&Release>,
    mut plan: Plan,
    version: &str,
    licenses: bool,
) -> Result<Option<(PixelWindow, Chosen)>> {
    let fresh = plan.kind == Kind::Fresh;
    let state = if fresh { None } else { Some(read_install_state(&plan.root)?) };
    let namespace = match (&state, release) {
        (Some(state), _) => state.release_descriptor.bundle_namespace.clone(),
        (None, Some(release)) => release.manifest.bundle_namespace.clone(),
        (None, None) => anyhow::bail!("nothing to install or move"),
    };
    let on_trial = matches!(plan.kind, Kind::Upgrade { .. } | Kind::Downgrade { .. })
        || state.as_ref().is_some_and(|state| state.pending.is_some());
    let locations = data_choice::locations(host, &namespace, state.as_ref())?;
    let mut data = DataChoice::Keep;
    let mut data_shown = data_choice::shown(&locations);
    let mut move_to: Option<Relocation> = None;
    let (mut changed, mut shortcut) = (false, false);
    let (mut error, mut data_error) = (None::<String>, None::<String>);
    let page_for = |plan: &Plan,
                    move_to: &Option<Relocation>,
                    data_shown: &PathBuf,
                    data: &DataChoice,
                    changed: bool,
                    shortcut: bool,
                    error: &Option<String>,
                    data_error: &Option<String>|
     -> Page {
        let root = move_to.as_ref().map_or(&plan.root, |relocation| &relocation.to);
        pages::location(&pages::Location {
            kind: &plan.kind,
            version,
            root,
            changed,
            licenses,
            shortcut,
            error: error.as_deref(),
            data: data_shown,
            data_changed: *data != DataChoice::Keep,
            data_error: data_error.as_deref(),
        })
    };
    let mut page =
        page_for(&plan, &move_to, &data_shown, &data, changed, shortcut, &error, &data_error);
    let mut window = PixelWindow::open(&page)?;
    loop {
        let action = window.show(&mut page)?;
        shortcut = page.checked("shortcut");
        match action {
            Action::Button("go") => break,
            Action::Button("change") => {
                if let Some(picked) = outside.choose_folder(&window, "选择安装 Aura 的文件夹")
                {
                    let root = root_in(&picked);
                    match (fresh, release) {
                        (true, Some(release)) => match install::plan(host, release, Some(root)) {
                            Ok(chosen) => {
                                plan = chosen;
                                changed = true;
                                error = None;
                            }
                            Err(refused) => error = Some(format!("{refused:#}")),
                        },
                        _ => match relocate::plan(host, &plan.root, &root) {
                            Ok(relocation) => {
                                move_to = Some(relocation);
                                changed = true;
                                error = None;
                            }
                            Err(refusals) => {
                                error = Some(format!(
                                    "不能移到 {}：{}",
                                    root.display(),
                                    folder_check::joined(&refusals)
                                ));
                            }
                        },
                    }
                }
            }
            Action::Button("change-data") => {
                if let Some(picked) = outside.choose_folder(&window, "选择放数据的文件夹")
                {
                    let root = move_to.as_ref().map_or(&plan.root, |relocation| &relocation.to);
                    match data_choice::choose(
                        host, &namespace, &locations, &picked, root, fresh, on_trial,
                    ) {
                        Ok(choice) => {
                            data_shown = match &choice {
                                DataChoice::Keep => data_choice::shown(&locations),
                                DataChoice::Reuse(folder)
                                | DataChoice::Start(folder)
                                | DataChoice::Move(folder) => folder.clone(),
                            };
                            data = choice;
                            data_error = None;
                        }
                        Err(reasons) => {
                            data_error = Some(format!(
                                "不能放在 {}：{}",
                                picked.display(),
                                reasons.join("；")
                            ));
                        }
                    }
                }
            }
            Action::Link("licenses") => outside.show_licenses(program),
            _ => return Ok(None),
        }
        page =
            page_for(&plan, &move_to, &data_shown, &data, changed, shortcut, &error, &data_error);
    }
    Ok(Some((window, Chosen { plan, move_to, data, shortcut })))
}
