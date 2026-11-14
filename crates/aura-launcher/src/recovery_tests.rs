//! Every launcher window drawn offscreen at 100, 125, 150 and 200 % through the pixel windows'
//! checks; the buttons each window offers and which one Enter takes; and the keyboard alone.
#![allow(clippy::unwrap_used)]
use std::{path::PathBuf, sync::Mutex, thread, time::Duration};

use aura_data_location::LocationError;
use aura_pixel_window::{
    Kind, Page, Snapshot,
    audit::{audit, missing_glyphs},
    draw, fonts,
    scale::whole_scale,
    testing::{Key, hide_windows, parts, press, wait_for_heading, wait_for_window},
};
use aura_windows_platform::DriveKind;

use super::{
    Recovery, RecoveryAction, ask, backup_failed, crash_loop, fatal, progress_page, recovery_page,
    startup_failed,
};
use crate::{data_location, data_move};

const DPIS: [u32; 4] = [96, 120, 144, 192];

fn every_window() -> Vec<(&'static str, Page)> {
    let folder = PathBuf::from(r"D:\AuraData");
    let missing = LocationError::LocationMissing {
        location: folder.clone(),
        reason: "系统找不到指定的路径。 (os error 3)".to_owned(),
    };
    let not_found = |error: LocationError| recovery_page(&data_location::window(&error));
    let slimming =
        Snapshot {
            done: 1, total: 4, stage: Some("删除旧记录".to_owned()), ..Snapshot::default()
        };
    let tidying = Snapshot {
        stage: Some("整理数据库文件".to_owned()),
        unmeasured: true,
        ..Snapshot::default()
    };
    let copying = Snapshot {
        done: 3_430_940_672,
        total: 9_028_239_360,
        elapsed: Duration::from_secs(25),
        ..Snapshot::default()
    };
    let (from, to) = (r"C:\Users\14153\AppData\Local\Aura\namespaces\961d23db", r"D:\AuraData");
    let verifying = Snapshot { stage: Some("核对复制的文件".to_owned()), ..copying.clone() };
    let stores =
        Snapshot { stage: Some("检查数据库".to_owned()), unmeasured: true, ..copying.clone() };
    let stopping = Snapshot { stop_asked: true, ..copying.clone() };
    vec![
        ("waiting", progress_page("正在整理数据", &Snapshot::default())),
        ("slimming", progress_page("正在整理数据", &slimming)),
        ("tidying", progress_page("正在整理数据", &tidying)),
        ("backing-up", progress_page("正在备份更新前的数据", &copying)),
        ("moving", data_move::page(from, to, &copying, true)),
        ("moving-verifying", data_move::page(from, to, &verifying, true)),
        ("moving-stores", data_move::page(from, to, &stores, false)),
        ("moving-stopping", data_move::page(from, to, &stopping, true)),
        (
            "backup-no-room",
            recovery_page(&backup_failed(
                "0.3.0",
                "磁盘空间不够备份数据",
                "需要约 5,040 MB，这个磁盘只剩 812 MB。",
            )),
        ),
        (
            "startup-failed",
            recovery_page(&startup_failed(
                "0.3.0",
                "aura-core.exe 与发布清单不一致",
                Some("0.2.0"),
            )),
        ),
        (
            "startup-failed-alone",
            recovery_page(&startup_failed("0.3.0", "active.json 读不出", None)),
        ),
        ("crash-loop", recovery_page(&crash_loop("desktop", 3221225477, "0.3.0", Some("0.2.0")))),
        (
            "fatal",
            recovery_page(&fatal(r"failed to read D:\Aura\active.json: 拒绝访问。 (os error 5)")),
        ),
        ("not-found", not_found(missing.clone())),
        ("not-found-released", not_found(LocationError::Production(Box::new(missing)))),
        (
            "not-found-record",
            not_found(LocationError::RecordUnreadable {
                path: PathBuf::from(
                    r"C:\Users\14153\AppData\Local\Aura\namespaces\961d23db\location.json",
                ),
                reason: "expected value at line 1 column 1".to_owned(),
            }),
        ),
        (
            "not-found-disagree",
            not_found(LocationError::RecordsDisagree {
                record: Some(folder.clone()),
                install_copy: None,
            }),
        ),
        (
            "not-found-marker",
            not_found(LocationError::MarkerMismatch {
                location: folder.clone(),
                reason: "aura-data.v1.json names another installation".to_owned(),
            }),
        ),
        (
            "not-found-removable",
            not_found(LocationError::NotFixedDisk { location: folder, kind: DriveKind::Removable }),
        ),
    ]
}

#[test]
fn every_launcher_window_passes_the_checks_at_each_scale() {
    let shots = std::env::var_os("AURA_PIXEL_SHOTS").map(PathBuf::from);
    let fonts = fonts().unwrap();
    for (name, page) in every_window() {
        for dpi in DPIS {
            let k = whole_scale(dpi);
            let (laid, image) = draw(&page, k).unwrap();
            if let Some(folder) = &shots {
                let _ = std::fs::create_dir_all(folder);
                let _ = image.save_png(&folder.join(format!("launcher-{name}-{dpi}dpi.png")));
            }
            let mut found = audit(&laid, &image, k);
            found.extend(missing_glyphs(&laid, &fonts));
            assert!(found.is_empty(), "{name} at {dpi} DPI: {found:#?}");
        }
    }
}

type Buttons = Vec<(String, Kind)>;

/// The footer as (label, kind) at its left edge and at its right.
fn footer(recovery: &Recovery<'_>) -> (Buttons, Buttons) {
    let page = recovery_page(recovery);
    let shown = |buttons: &[aura_pixel_window::Button]| {
        buttons.iter().map(|button| (button.label.clone(), button.kind)).collect::<Vec<_>>()
    };
    (shown(&page.footer.start), shown(&page.footer.end))
}

fn button(label: &str, kind: Kind) -> (String, Kind) {
    (label.to_owned(), kind)
}

#[test]
fn leaving_stands_apart_and_enter_takes_the_first_choice_that_keeps_the_data() {
    let crash = footer(&crash_loop("desktop", 1, "0.3.0", Some("0.2.0")));
    assert_eq!(crash.0, [button("退出", Kind::Normal)]);
    assert_eq!(
        crash.1,
        [
            button("导出诊断", Kind::Normal),
            button("回到 0.2.0", Kind::Normal),
            button("安全模式", Kind::Normal),
            button("重新启动", Kind::Primary),
        ]
    );
    let without_previous = footer(&crash_loop("desktop", 1, "0.3.0", None));
    assert!(
        !without_previous.1.iter().any(|(label, _)| label.starts_with("回到")),
        "{without_previous:?}"
    );
    assert_eq!(
        footer(&startup_failed("0.3.0", "", Some("0.2.0"))),
        (vec![button("退出", Kind::Normal)], vec![button("回到 0.2.0", Kind::Primary)])
    );
    assert_eq!(
        footer(&startup_failed("0.3.0", "", None)),
        (vec![], vec![button("退出", Kind::Primary)])
    );
    assert_eq!(
        footer(&backup_failed("0.3.0", "磁盘空间不够备份数据", "")),
        (vec![button("退出", Kind::Normal)], vec![button("不备份，启动新版本", Kind::Danger)])
    );
    assert_eq!(
        footer(&data_location::window(&LocationError::Environment("LOCALAPPDATA"))),
        (vec![button("退出", Kind::Normal)], vec![button("重试", Kind::Primary)])
    );
}

#[test]
fn a_progress_window_names_no_figures_before_the_work_reports_any() {
    let meter = |page: Page| match page.blocks.into_iter().next() {
        Some(aura_pixel_window::Block::Meter(meter)) => meter,
        other => panic!("the window holds no bar: {other:?}"),
    };
    for heading in ["正在整理数据", "正在备份更新前的数据"] {
        let waiting = meter(progress_page(heading, &Snapshot::default()));
        assert_eq!(
            (waiting.fraction, &*waiting.left, &*waiting.right),
            (None, "", ""),
            "{heading} before the first report"
        );
    }
    let copying = meter(progress_page(
        "正在备份更新前的数据",
        &Snapshot { done: 0, total: 9_028_239_360, ..Snapshot::default() },
    ));
    assert_eq!(copying.left, "已复制 0 MB / 8,610 MB");
}

#[test]
fn the_move_window_says_from_where_to_where_and_offers_to_stop_only_while_it_can() {
    let copying = Snapshot { done: 1 << 30, total: 4 << 30, ..Snapshot::default() };
    let shown = |page: &Page| {
        let fields: Vec<(String, String)> = page
            .blocks
            .iter()
            .filter_map(|block| match block {
                aura_pixel_window::Block::Field(field) => {
                    Some((field.label.clone(), field.value.clone()))
                }
                _ => None,
            })
            .collect();
        let stop: Vec<(String, bool)> =
            page.footer.end.iter().map(|button| (button.label.clone(), button.enabled)).collect();
        (page.heading.clone(), fields, stop)
    };
    let rows = vec![("从".to_owned(), r"C:\A".to_owned()), ("到".to_owned(), r"D:\B".to_owned())];
    assert_eq!(
        shown(&data_move::page(r"C:\A", r"D:\B", &copying, true)),
        ("正在搬数据".to_owned(), rows.clone(), vec![("取消".to_owned(), true)])
    );
    let asked = Snapshot { stop_asked: true, ..copying.clone() };
    assert_eq!(
        shown(&data_move::page(r"C:\A", r"D:\B", &asked, true)),
        ("正在停止".to_owned(), rows.clone(), vec![("取消".to_owned(), false)])
    );
    assert_eq!(
        shown(&data_move::page(r"C:\A", r"D:\B", &copying, false)),
        ("正在搬数据".to_owned(), rows, vec![]),
        "once the stores are checked the move cannot stop"
    );
}

#[test]
fn a_store_check_that_fails_names_each_store_that_failed() {
    let answer =
        "{\"data/aura.db\":true,\"data/v2-execution/aura-v2.db\":{\"error\":\"malformed\"}}\n";
    assert_eq!(
        data_move::store_check_failure(answer, Some(1)),
        "data/v2-execution/aura-v2.db：malformed"
    );
    assert_eq!(data_move::store_check_failure("", Some(2)), "Core 的检查以 2 结束");
    assert_eq!(data_move::store_check_failure("not json", None), "Core 的检查以 未知代码 结束");
}

static WINDOWS: Mutex<()> = Mutex::new(());

/// Asks on a thread of its own, presses `keys` once the window shows `heading`, and returns
/// the answer and what a screen reader read on the window.
fn answered(
    recovery: Recovery<'static>,
    keys: &[Key],
) -> (RecoveryAction, Vec<(&'static str, String)>) {
    hide_windows();
    let _one = WINDOWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let heading = recovery.heading;
    let asking = thread::spawn(move || ask(&recovery).unwrap());
    let window = wait_for_window(Duration::from_secs(20)).expect("the window appears");
    assert!(wait_for_heading(window, heading, Duration::from_secs(20)));
    let read = parts(window).unwrap().into_iter().map(|part| (part.role, part.name)).collect();
    for key in keys {
        press(window, *key);
    }
    (asking.join().unwrap(), read)
}

#[test]
fn the_keyboard_alone_answers_and_a_screen_reader_reads_the_window() {
    let missing = LocationError::LocationMissing {
        location: PathBuf::from(r"D:\AuraData"),
        reason: "系统找不到指定的路径。".to_owned(),
    };
    let (action, read) = answered(data_location::window(&missing), &[Key::Enter]);
    assert_eq!(action, RecoveryAction::Retry);
    for wanted in [
        ("text", "找不到数据位置"),
        ("edit", "数据位置"),
        ("text", "数据文件夹不在"),
        ("text", "系统找不到指定的路径。"),
        ("button", "退出"),
        ("button", "重试"),
        ("button", "关闭"),
    ] {
        assert!(
            read.iter().any(|(role, name)| *role == wanted.0 && name == wanted.1),
            "{wanted:?} is not among {read:#?}"
        );
    }

    // The danger choice never takes Enter: Enter leaves, Tab then Enter starts without the copy.
    let backup =
        || backup_failed("0.3.0", "磁盘空间不够备份数据", "需要约 5,040 MB，这个磁盘只剩 812 MB。");
    assert_eq!(answered(backup(), &[Key::Enter]).0, RecoveryAction::Exit);
    assert_eq!(answered(backup(), &[Key::Tab, Key::Enter]).0, RecoveryAction::StartWithoutBackup);
    assert_eq!(answered(fatal("拒绝访问。"), &[Key::Escape]).0, RecoveryAction::Exit);
}
