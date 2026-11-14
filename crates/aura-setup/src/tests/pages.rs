//! Every page of the setup and uninstall windows drawn offscreen at 100, 125, 150 and 200 %,
//! through the pixel windows' checks: type sizes, square corners, hard shadows, icon sizes, no
//! coloured edges, and every pixel-face word in the face.
use std::{
    path::PathBuf,
    time::{Duration, UNIX_EPOCH},
};

use aura_old_data::{Kind as OldKind, OldData, Removed};
use aura_pixel_window::{
    Page, Snapshot,
    audit::{audit, missing_glyphs},
    draw, fonts,
    scale::whole_scale,
};

use time::UtcOffset;

use super::namespace;
use crate::{
    install::Kind,
    uninstall::UninstallPlan,
    wizard::{finish::removal_notes, pages},
};

const DPIS: [u32; 4] = [96, 120, 144, 192];
const UTC: UtcOffset = UtcOffset::UTC;

fn plan() -> UninstallPlan {
    UninstallPlan {
        root: PathBuf::from(r"D:\Aura"),
        namespace: namespace(),
        version: "0.3.0".to_owned(),
        program_bytes: 1_782_579_200,
        data_root: PathBuf::from(r"C:\Users\14153\AppData\Local\Aura\namespaces\961d23db\data"),
        data_bytes: 9_244_917_000,
        config_root: PathBuf::from(r"C:\Users\14153\AppData\Roaming\Aura\namespaces\961d23db"),
        config_bytes: 48_000,
        credentials: 3,
        chrome_root: PathBuf::from(r"C:\Users\14153\AppData\Local\Aura\namespaces\961d23db\chrome"),
        moved_data: Vec::new(),
    }
}

/// The early layout, the 875 MB namespace and a removal that did not finish, as on the owner's
/// computer before 0.3.0.
fn old_data() -> Vec<OldData> {
    let day = |seconds| Some(UNIX_EPOCH + Duration::from_secs(seconds));
    let local = PathBuf::from(r"C:\Users\14153\AppData\Local\Aura");
    vec![
        OldData {
            id: "early".to_owned(),
            kind: OldKind::EarlyLayout,
            paths: vec![local.join("data"), local.join("desktop")],
            bytes: 52_000,
            modified: day(1_788_259_029),
        },
        OldData {
            id: "namespace-961d23db09870b4258ab1f48d367ca5d".to_owned(),
            kind: OldKind::Namespace,
            paths: vec![local.join(r"namespaces\961d23db09870b4258ab1f48d367ca5d")],
            bytes: 917_294_454,
            modified: day(1_790_238_669),
        },
        OldData {
            id: "unfinished".to_owned(),
            kind: OldKind::Unfinished,
            paths: vec![local.join("data.1762480000000.aura-removing")],
            bytes: 4_096,
            modified: None,
        },
    ]
}

fn every_page() -> Vec<(&'static str, Page)> {
    let fresh = Kind::Fresh;
    let upgrade = Kind::Upgrade { from: "0.2.0".to_owned() };
    let root = PathBuf::from(r"C:\Users\14153\AppData\Local\Programs\Aura");
    let data = PathBuf::from(r"C:\Users\14153\AppData\Local\Aura\namespaces\a47182cd");
    let location = |kind, changed, error| pages::Location {
        kind,
        version: "0.3.0",
        root: &root,
        changed,
        licenses: true,
        shortcut: false,
        error,
        data: &data,
        data_changed: false,
        data_error: None,
    };
    let moved_data = PathBuf::from(r"D:\Aura 数据");
    let relocating = pages::Location {
        kind: &Kind::Relocate,
        version: "0.3.0",
        root: &root,
        changed: false,
        licenses: false,
        shortcut: false,
        error: None,
        data: &moved_data,
        data_changed: true,
        data_error: Some(r"不能放在 D:\OneDrive：在 OneDrive 的同步文件夹里"),
    };
    let notes = vec![
        "任务栏上固定的 Aura 没能改到新位置，请重新固定".to_owned(),
        r"下次启动 Aura 时，数据搬到 D:\Aura 数据".to_owned(),
    ];
    let copying = Snapshot {
        done: 1_048_576_000,
        total: 1_782_579_200,
        elapsed: Duration::from_secs(12),
        ..Snapshot::default()
    };
    let checking = Snapshot { done: copying.total, ..copying.clone() };
    let refused = r"D:\Aura 不是空文件夹，请选择一个空文件夹或新文件夹。";
    vec![
        ("setup-location", pages::location(&location(&fresh, false, None))),
        ("setup-location-chosen", pages::location(&location(&fresh, true, None))),
        ("setup-location-refused", pages::location(&location(&fresh, false, Some(refused)))),
        ("setup-location-upgrade", pages::location(&location(&upgrade, false, None))),
        ("setup-location-repair", pages::location(&location(&Kind::Repair, false, None))),
        ("setup-running", pages::running(pages::running_frame(&upgrade, "0.3.0"))),
        ("setup-starting", pages::working(&fresh, "0.3.0", &Snapshot::default())),
        ("setup-copying", pages::working(&upgrade, "0.3.0", &copying)),
        ("setup-checking", pages::working(&upgrade, "0.3.0", &checking)),
        ("setup-installed", pages::finished(&fresh, "0.3.0", &[], &[], &[], true, UTC)),
        ("setup-updated", pages::finished(&upgrade, "0.3.0", &[], &[], &[], true, UTC)),
        (
            "setup-updated-old-data",
            pages::finished(&upgrade, "0.3.0", &[], &old_data(), &[true, true, true], true, UTC),
        ),
        (
            "setup-updated-old-data-left",
            pages::finished(
                &upgrade,
                "0.3.0",
                &[
                    "已删除：早期版本的数据".to_owned(),
                    "旧安装的数据 961d23db 正在使用，没有删".to_owned(),
                ],
                &old_data()[1..],
                &[false, false],
                false,
                UTC,
            ),
        ),
        ("setup-removing-old-data", pages::removing_old(&upgrade, "0.3.0")),
        ("setup-relocate", pages::location(&relocating)),
        ("setup-moving", pages::moving(&Kind::Relocate, "0.3.0", 0, &copying)),
        ("setup-moving-registering", pages::moving(&Kind::Relocate, "0.3.0", 2, &checking)),
        ("setup-moved", pages::finished(&Kind::Relocate, "0.3.0", &notes, &[], &[], true, UTC)),
        (
            "setup-failed",
            pages::failed(
                &upgrade,
                "0.3.0",
                "aura-core.exe 与发布清单不一致：sha256 不同",
                Some("已安装的 Aura 没有改变。"),
            ),
        ),
        ("setup-cannot-install", pages::message("", "无法安装", "这个安装程序里没有可安装的版本")),
        ("uninstall-confirm", pages::confirm_uninstall(&plan(), false)),
        ("uninstall-confirm-data", pages::confirm_uninstall(&plan(), true)),
        ("uninstall-running", pages::uninstall_running(&plan())),
        ("uninstall-working", pages::uninstalling(&plan())),
        ("uninstall-done", pages::uninstalled(&plan(), false)),
        ("uninstall-done-data", pages::uninstalled(&plan(), true)),
        (
            "uninstall-failed",
            pages::uninstall_failed(&plan(), r"cannot remove D:\Aura: 拒绝访问。 (os error 5)"),
        ),
    ]
}

#[test]
fn every_setup_and_uninstall_page_passes_the_checks_at_each_scale() {
    let shots = std::env::var_os("AURA_PIXEL_SHOTS").map(PathBuf::from);
    let fonts = fonts().expect("fonts");
    for (name, page) in every_page() {
        for dpi in DPIS {
            let k = whole_scale(dpi);
            let (laid, image) = draw(&page, k).expect("the page draws");
            if let Some(folder) = &shots {
                let _ = std::fs::create_dir_all(folder);
                let _ = image.save_png(&folder.join(format!("{name}-{dpi}dpi.png")));
            }
            let mut found = audit(&laid, &image, k);
            found.extend(missing_glyphs(&laid, &fonts));
            assert!(found.is_empty(), "{name} at {dpi} DPI: {found:#?}");
        }
    }
}

#[test]
fn the_old_data_is_ticked_at_first_named_with_size_and_day_and_removed_only_when_picked() {
    let page =
        pages::finished(&Kind::Fresh, "0.3.0", &[], &old_data(), &[true, true, true], true, UTC);
    let boxes: Vec<(String, bool)> = page
        .blocks
        .iter()
        .filter_map(|block| match block {
            aura_pixel_window::Block::Check(check) if check.id.starts_with("old-") => {
                Some((check.label.clone(), check.checked))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        boxes,
        [
            ("早期版本的数据 · < 0.1 MB · 2026-09-01".to_owned(), true),
            ("旧安装的数据 961d23db · 875 MB · 2026-09-24".to_owned(), true),
            ("没删完的旧数据 · < 0.1 MB".to_owned(), true),
        ]
    );
    let remove = |page: &Page| {
        let button = page.buttons().find(|button| button.id == "remove-old").expect("删除所选");
        (button.label.clone(), button.kind, button.enabled)
    };
    assert_eq!(remove(&page), ("删除所选".to_owned(), aura_pixel_window::Kind::Danger, true));
    let none = pages::finished(&Kind::Fresh, "0.3.0", &[], &old_data(), &[false; 3], true, UTC);
    assert!(!remove(&none).2, "删除所选 is pressed with nothing ticked");
    let nothing = pages::finished(&Kind::Fresh, "0.3.0", &[], &[], &[], true, UTC);
    assert!(nothing.buttons().all(|button| button.id != "remove-old"));
    assert!(nothing.blocks.iter().all(|block| !matches!(
        block,
        aura_pixel_window::Block::Text(line) if line.text == "旧数据"
    )));
}

#[test]
fn an_items_day_is_the_day_on_the_users_clock() {
    // 23:36 UTC on 6 November is 07:36 on 7 November in China.
    let mut item = old_data().swap_remove(1);
    item.modified = Some(UNIX_EPOCH + Duration::from_secs(1_794_008_206));
    let label = |offset| {
        let page =
            pages::finished(&Kind::Repair, "0.3.0", &[], &[item.clone()], &[true], true, offset);
        page.blocks
            .iter()
            .find_map(|block| match block {
                aura_pixel_window::Block::Check(check) if check.id == "old-0" => {
                    Some(check.label.clone())
                }
                _ => None,
            })
            .expect("the item is listed")
    };
    let china = UtcOffset::from_hms(8, 0, 0).expect("UTC+8");
    assert_eq!(label(china), "旧安装的数据 961d23db · 875 MB · 2026-11-07");
    assert_eq!(label(UTC), "旧安装的数据 961d23db · 875 MB · 2026-11-06");
}

#[test]
fn the_notes_after_a_removal_name_what_went_what_is_in_use_and_what_did_not_finish() {
    let old = old_data();
    let namespace = old[1].id.clone();
    let went_and_in_use = Removed {
        removed: vec!["early".to_owned(), "unfinished".to_owned()],
        in_use: vec![namespace.clone()],
        failed: Vec::new(),
    };
    assert_eq!(
        removal_notes(&went_and_in_use, &old),
        ["已删除：早期版本的数据、没删完的旧数据", "旧安装的数据 961d23db 正在使用，没有删"]
    );
    let cut_short = Removed {
        failed: vec![(namespace, "拒绝访问。 (os error 5)".to_owned())],
        ..Removed::default()
    };
    assert_eq!(
        removal_notes(&cut_short, &old),
        ["旧安装的数据 961d23db 没删完：拒绝访问。 (os error 5)"]
    );
    assert!(removal_notes(&Removed::default(), &old).is_empty());
}

#[test]
fn the_uninstall_page_names_what_goes_and_ticking_the_box_turns_the_button_to_danger() {
    let kept = pages::confirm_uninstall(&plan(), false);
    let deleting = pages::confirm_uninstall(&plan(), true);
    let remove = |page: &Page| {
        let button =
            page.buttons().find(|button| button.id == "remove").expect("the remove button");
        (button.label.clone(), button.kind)
    };
    assert_eq!(remove(&kept), ("卸载".to_owned(), aura_pixel_window::Kind::Primary));
    assert_eq!(remove(&deleting), ("删除并卸载".to_owned(), aura_pixel_window::Kind::Danger));
    let (laid, _) = draw(&deleting, 1).expect("draws");
    let rows: Vec<&str> = laid
        .nodes
        .iter()
        .filter(|node| node.automation_id.starts_with("pair-"))
        .map(|node| &*node.name)
        .collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    for (row, (key, size)) in
        rows.iter().zip([("程序", "1.66 GB"), ("数据", "8.61 GB"), ("设置", "< 0.1 MB")])
    {
        assert!(row.starts_with(key) && row.contains(size), "{row:?} should name {key} and {size}");
    }
}
