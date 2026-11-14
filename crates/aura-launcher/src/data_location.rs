//! Where the launcher finds the installation's data, and what happens when the records name a
//! folder that cannot be used: nothing starts, and the user can look again, point Aura at the
//! folder the data is in now, or close.
use std::path::Path;

use anyhow::{Context, Result};
use aura_data_location::{InstallCopy, LocationError, Locations, Roots};
use aura_pixel_window::{Block, Field, Tone};
use aura_runtime_contract::ActiveInstallState;

use crate::{
    install_state,
    recovery::{self, Recovery, RecoveryAction, quoted, words},
};

/// The locations `state`'s installation uses, from its location record and `active.json`'s copy.
pub(crate) fn locate(state: &ActiveInstallState) -> Result<Locations, LocationError> {
    let roots = Roots::from_system()?;
    Locations::resolve(&roots, &state.release_descriptor.bundle_namespace, &InstallCopy::of(state))
}

/// Says which folder is missing and why, until the user looks again or points Aura at the folder
/// the data is in now (`true`), or closes (`false`).
pub(crate) fn not_found(
    root: &Path,
    state: &ActiveInstallState,
    error: &LocationError,
) -> Result<bool> {
    let mut error = error.clone();
    loop {
        let action =
            recovery::ask(&window(&error)).context("failed to show the data location window")?;
        match action {
            RecoveryAction::Retry => return Ok(true),
            RecoveryAction::ChooseLocation => {
                let chosen = aura_pixel_window::choose_folder(None, "选择数据所在的文件夹")
                    .context("failed to show the folder picker")?;
                if let Some(folder) = chosen {
                    match point_at(root, state, &folder) {
                        Ok(()) => return Ok(true),
                        Err(refused) => error = refused,
                    }
                }
            }
            _ => return Ok(false),
        }
    }
}

/// Points the location record and the installation's copy at `folder`, which must hold this
/// installation's data.
fn point_at(root: &Path, state: &ActiveInstallState, folder: &Path) -> Result<(), LocationError> {
    let roots = Roots::from_system()?;
    aura_data_location::point_at(&roots, &state.release_descriptor.bundle_namespace, folder)?;
    let active = root.join("active.json");
    let unwritten = |error: anyhow::Error| LocationError::RecordUnreadable {
        path: active.clone(),
        reason: format!("{error:#}"),
    };
    let mut state = install_state::read(root).map_err(unwritten)?;
    state.data_location = Some(folder.display().to_string());
    install_state::write(root, &state).map_err(unwritten)
}

pub(crate) fn window(error: &LocationError) -> Recovery<'static> {
    let actions: &'static [RecoveryAction] = match error {
        LocationError::Environment(_) | LocationError::KnownFolder(_) => {
            &[RecoveryAction::Retry, RecoveryAction::Exit]
        }
        _ => &[RecoveryAction::Retry, RecoveryAction::ChooseLocation, RecoveryAction::Exit],
    };
    Recovery {
        heading: "找不到数据位置", blocks: describe(error), actions, previous_version: None
    }
}

fn folder(label: &str, show_label: bool, path: Option<&Path>) -> Block {
    Block::Field(Field {
        label: label.to_owned(),
        show_label,
        value: path.map_or_else(|| "没有记".to_owned(), |path| path.display().to_string()),
        changed: false,
        button: None,
        detail: Vec::new(),
    })
}

/// The folder the error is about, what is wrong with it, and what the system said.
pub(crate) fn describe(error: &LocationError) -> Vec<Block> {
    match error {
        LocationError::RecordUnreadable { path, reason } => vec![
            folder("位置记录", false, Some(path)),
            words("位置记录读不出", Tone::Ink),
            quoted(reason.to_string()),
        ],
        LocationError::RecordsDisagree { record, install_copy } => vec![
            folder("位置记录", true, record.as_deref()),
            folder("安装文件夹里记的", true, install_copy.as_deref()),
            words("两份记录不一致", Tone::Ink),
        ],
        LocationError::LocationMissing { location, reason } => vec![
            folder("数据位置", false, Some(location)),
            words("数据文件夹不在", Tone::Ink),
            quoted(reason.to_string()),
        ],
        LocationError::MarkerMismatch { location, reason } => vec![
            folder("数据位置", false, Some(location)),
            words("这里不是这个安装的数据", Tone::Ink),
            quoted(reason.to_string()),
        ],
        LocationError::NotFixedDisk { location, .. } => {
            vec![
                folder("数据位置", false, Some(location)),
                words("不在本机的固定磁盘上", Tone::Ink),
            ]
        }
        LocationError::Production(inner) => describe(inner),
        LocationError::Environment(_) | LocationError::KnownFolder(_) => {
            vec![quoted(error.to_string())]
        }
    }
}
