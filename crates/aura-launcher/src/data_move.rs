//! Moving the data where the user asked, while Aura is closed (`aura_data_move`). A move a crash or
//! power loss cut short is finished or taken back before the data is looked for. A requested move
//! runs with a window that says from where to where and how far it got, and that can stop it while
//! it copies. Whatever happens, Aura then starts from wherever the data is; a move that did not
//! happen says why on the storage page.
use std::{
    io,
    path::Path,
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use aura_data_location::{Locations, Roots};
use aura_data_move::{Hooks, Outcome, Refusal, Stage};
use aura_pixel_window::{Block, Button, Field, Kind, Page, Progress, STOP, Snapshot};
use aura_runtime_contract::{ActiveInstallState, PRODUCTION_BUNDLE_NAMESPACE};
use serde_json::Value;

use crate::{install_state, recovery};

const HEADING: &str = "正在搬数据";
const STOPPING: &str = "正在停止";
const VERIFYING: &str = "核对复制的文件";

/// Finishes or takes back a move that stopped part way. Runs before the data is looked for, since
/// a move cut short while its records were written leaves records that disagree.
pub(crate) fn resume(root: &Path, state: &ActiveInstallState) -> Outcome {
    let Ok(roots) = Roots::from_system() else {
        return Outcome::NothingAsked;
    };
    let namespace = &state.release_descriptor.bundle_namespace;
    let install_copy = |location: Option<&Path>| write_install_copy(root, location);
    let hooks = Hooks {
        progress: &|_, _, _| {},
        cancelled: &|| false,
        check_stores: &|_| Ok(()),
        install_copy: &install_copy,
        production: namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE,
        fault: None,
    };
    aura_data_move::resume(&roots, namespace, &hooks)
}

/// Does the move the user asked for, if any. `core` is the version's Core, which checks the copy's
/// stores. A version on trial moves nothing: the request is taken back and says why.
pub(crate) fn requested(
    root: &Path,
    state: &ActiveInstallState,
    locations: &Locations,
    core: &Path,
    trial: bool,
) -> Outcome {
    let namespace = &state.release_descriptor.bundle_namespace;
    let Ok(Some(destination)) = aura_data_move::requested(namespace, locations) else {
        return Outcome::NothingAsked;
    };
    let declined = |reason: String| {
        let _ = aura_data_move::decline(namespace, locations, reason.clone());
        Outcome::Failed { reason }
    };
    if trial {
        return declined(Refusal::OnTrial.to_string());
    }
    let roots = match Roots::from_system() {
        Ok(roots) => roots,
        Err(error) => return declined(error.to_string()),
    };
    let current = match locations.clone().with_global_memory(&roots, namespace) {
        Ok(current) => current,
        Err(error) => return declined(error.to_string()),
    };
    let shown = |place: Option<&Path>| {
        place.map_or_else(
            || Locations::for_place(&roots, namespace, None).data_root.display().to_string(),
            |place| place.display().to_string(),
        )
    };
    let (from, to) = (shown(current.location.as_deref()), shown(destination.as_deref()));
    let stoppable = Arc::new(AtomicBool::new(true));
    let page_stoppable = Arc::clone(&stoppable);
    aura_pixel_window::with_progress_window(
        move |snapshot| page(&from, &to, snapshot, page_stoppable.load(Ordering::Acquire)),
        |progress| {
            let reported = |stage: Stage, done: u64, total: u64| {
                report(progress, &stoppable, stage, done, total);
            };
            let cancelled = || progress.stop_asked();
            let check_stores = |data_root: &Path| check_stores(core, data_root);
            let install_copy = |location: Option<&Path>| write_install_copy(root, location);
            let hooks = Hooks {
                progress: &reported,
                cancelled: &cancelled,
                check_stores: &check_stores,
                install_copy: &install_copy,
                production: namespace.as_str() == PRODUCTION_BUNDLE_NAMESPACE,
                fault: None,
            };
            aura_data_move::perform(&roots, namespace, &current, &hooks)
        },
    )
}

fn report(progress: &Progress, stoppable: &AtomicBool, stage: Stage, done: u64, total: u64) {
    match stage {
        Stage::Copying => {
            progress.set_total(total);
            progress.set_done(done);
        }
        Stage::Verifying => {
            progress.set_stage(VERIFYING, false);
            progress.set_total(total);
            progress.set_done(done);
        }
        // From here on the copy is checked as a whole and the move cannot stop.
        Stage::CheckingStores => {
            stoppable.store(false, Ordering::Release);
            progress.set_stage("检查数据库", true);
        }
        Stage::SettingAccess => progress.set_stage("设置权限", true),
        Stage::Recording => progress.set_stage("记下新位置", true),
    }
}

/// The move's window: from where to where, how far it got, and 取消 while it can stop.
pub(crate) fn page(from: &str, to: &str, snapshot: &Snapshot, stoppable: bool) -> Page {
    let heading = if snapshot.stop_asked { STOPPING } else { HEADING };
    let mut page = recovery::progress_page(heading, snapshot);
    let row = |label: &str, value: &str| {
        Block::Field(Field {
            label: label.to_owned(),
            show_label: true,
            value: value.to_owned(),
            changed: false,
            button: None,
            detail: Vec::new(),
        })
    };
    page.blocks.splice(0..0, [row("从", from), row("到", to)]);
    if stoppable {
        let mut stop = Button::new(STOP, "取消", Kind::Normal);
        stop.enabled = !snapshot.stop_asked;
        page.footer.end = vec![stop];
    }
    page
}

/// SQLite's quick check of the copy's stores, by the version's Core (`--store-check`).
fn check_stores(core: &Path, data_root: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let output = Command::new(core)
        .arg("--store-check")
        .arg(data_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|error| format!("Core 没有启动：{error}"))?;
    if output.status.success() {
        return Ok(());
    }
    Err(store_check_failure(&String::from_utf8_lossy(&output.stdout), output.status.code()))
}

/// The stores that failed and why, from `--store-check`'s answer.
pub(crate) fn store_check_failure(answer: &str, code: Option<i32>) -> String {
    let failed: Vec<String> = answer
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok())
        .and_then(|answer| answer.as_object().cloned())
        .into_iter()
        .flatten()
        .filter_map(|(store, result)| {
            result["error"].as_str().map(|error| format!("{store}：{error}"))
        })
        .collect();
    if failed.is_empty() {
        format!(
            "Core 的检查以 {} 结束",
            code.map_or_else(|| "未知代码".to_owned(), |code| code.to_string())
        )
    } else {
        failed.join("；")
    }
}

/// The installation's copy of the location in `active.json`.
fn write_install_copy(root: &Path, location: Option<&Path>) -> io::Result<()> {
    let mut state = install_state::read(root).map_err(io::Error::other)?;
    state.data_location = location.map(|location| location.display().to_string());
    install_state::write(root, &state).map_err(io::Error::other)
}
