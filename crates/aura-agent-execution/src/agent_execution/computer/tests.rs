//! The checks Core makes on a computer call's own arguments and what it makes of the host's
//! reports. The calls themselves run with a played host in Core's tests.
#![allow(clippy::unwrap_used)]
use super::invoke::windows_on_image;
use super::resolve::{middle, region_on_screen, window};
use super::text;
use aura_desktop_domain::*;
use aura_desktop_service::KnownShot;

fn shot(map: ScreenMap) -> KnownShot {
    KnownShot { app_id: "notepad".into(), map, windows: vec![] }
}

#[test]
fn a_part_of_a_smaller_screenshot_is_the_screen_area_it_shows() {
    // 2560×1440 at (-2560, 0), shown as 1280×720: two screen pixels a screenshot pixel.
    let map = ScreenMap {
        left: -2560,
        top: 0,
        width: 1280,
        height: 720,
        screen_width: 2560,
        screen_height: 1440,
    };
    let region =
        |x, y, width, height| ComputerRegionInput { screenshot: "p1".into(), x, y, width, height };
    assert_eq!(
        region_on_screen(&shot(map), &region(100, 50, 200, 100)).unwrap(),
        Rect { x: -2360, y: 100, width: 400, height: 200 }
    );
    assert_eq!(
        region_on_screen(&shot(map), &region(1180, 620, 100, 100)).unwrap(),
        Rect { x: -200, y: 1240, width: 200, height: 200 },
        "up to the last pixel"
    );
    for outside in [region(1200, 0, 100, 10), region(0, 0, 0, 10), region(0, 700, 10, 21)] {
        let error = region_on_screen(&shot(map), &outside).unwrap_err().to_string();
        assert!(error.starts_with("computer.region"), "{error}");
    }
}

#[test]
fn a_window_left_of_and_below_the_primary_monitor_is_placed_on_a_smaller_screenshot() {
    // Three monitors of 5760 × 3600 together from (-1920, -1440), shown as 1568 × 980.
    let map = ScreenMap {
        left: -1920,
        top: -1440,
        width: 1568,
        height: 980,
        screen_width: 5760,
        screen_height: 3600,
    };
    let window = WindowInfo {
        window: "w1".into(),
        title: "记事本".into(),
        app_id: None,
        process_id: 1,
        process_name: "Notepad.exe".into(),
        rect: Rect { x: -1820, y: 700, width: 735, height: 367 },
        minimized: false,
        foreground: true,
    };
    let placed = &windows_on_image(&map, &[window])[0];
    assert_eq!((placed.x, placed.y, placed.width, placed.height), (27, 582, 200, 100));
}

#[test]
fn an_element_is_pointed_at_in_its_middle_and_a_window_by_its_ref() {
    assert_eq!(middle(Rect { x: 10, y: 20, width: 100, height: 31 }), Some(Point { x: 60, y: 35 }));
    assert_eq!(middle(Rect { x: 10, y: 20, width: 0, height: 31 }), None);
    assert_eq!(window(" w1312 ").unwrap(), "w1312");
    for wrong in ["1312", "w", "w0", "wx", "e12"] {
        assert!(window(wrong).unwrap_err().to_string().starts_with("computer.window"), "{wrong}");
    }
}

fn element(name: &str, role: Role, depth: u32) -> Element {
    Element {
        key: ElementKey {
            runtime_id: vec![42, depth as i32],
            control_type: 50_000,
            automation_id: String::new(),
            name: name.into(),
        },
        role,
        depth,
        rect: Rect { x: 0, y: 0, width: 10, height: 10 },
        value: None,
        enabled: true,
        focused: false,
        password: false,
        checked: None,
        expanded: None,
        selected: None,
        abilities: vec![],
        more: false,
    }
}

#[test]
fn a_read_is_one_line_an_element_indented_under_the_element_it_is_in() {
    let mut save = element("保存", Role::Button, 1);
    save.enabled = false;
    save.abilities = vec![Ability::Click, Ability::Focus];
    let mut edit = element("文字 \"a\"", Role::Edit, 1);
    edit.value = Some("第一行\n第二行".into());
    edit.focused = true;
    let mut check = element("自动换行", Role::CheckBox, 2);
    check.checked = Some(Checked::On);
    check.more = true;
    let elements = [element("记事本", Role::Window, 0), save, edit, check];
    let refs: Vec<String> = ["e1", "e2", "e3", "e4"].map(String::from).to_vec();
    assert_eq!(
        text::elements(&elements, &refs),
        [
            "e1 window \"记事本\"",
            "  e2 button \"保存\" disabled [click, focus]",
            "  e3 edit \"文字 \\\"a\\\"\" value=\"第一行\\n第二行\" focused",
            "    e4 check_box \"自动换行\" checked more",
        ]
        .join("\n")
    );
    assert_eq!(
        text::after(&refs[1..3], &[Some(elements[1].clone()), None]),
        "1. e2 button \"保存\" disabled [click, focus]\n2. e3 已经不在了（例如这一步关掉了它的窗口）"
    );
}

#[test]
fn a_window_action_acts_and_only_front_and_maximize_take_the_foreground() {
    let app =
        AppEntry {
            id: "notepad".into(), name: "记事本".into(), kind: AppKind::Manual, path: None
        };
    for (action, front) in [
        (WindowAction::Front, true),
        (WindowAction::Minimize, false),
        (WindowAction::Maximize, true),
        (WindowAction::Restore, false),
        (WindowAction::Close, false),
    ] {
        let command = HostCommand::Window { app: app.clone(), window: None, action };
        assert!(command.acts_on().is_some(), "{action:?}");
        assert_eq!(command.takes_foreground(), front, "{action:?}");
    }
}

#[test]
fn a_frozen_call_reads_back_the_same() {
    let app =
        AppEntry {
            id: "notepad".into(), name: "记事本".into(), kind: AppKind::Manual, path: None
        };
    let frozen = super::Frozen {
        mode: Default::default(),
        safety: None,
        command: HostCommand::Launch { app, timeout_ms: 15_000 },
        after: None,
        refs: vec![],
    };
    let value = serde_json::to_value(&frozen).unwrap();
    assert_eq!(value.as_object().unwrap().len(), 1, "only the command when nothing else: {value}");
    let back: super::Frozen = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(back).unwrap(), value);
}

fn listed(id: &str, name: &str, running: bool) -> aura_desktop_service::Listed {
    let entry = AppEntry { id: id.into(), name: name.into(), kind: AppKind::Packaged, path: None };
    aura_desktop_service::Listed { entry, running }
}

fn names(found: Vec<aura_desktop_service::Listed>) -> Vec<String> {
    found.into_iter().map(|known| known.entry.name).collect()
}

#[test]
fn a_search_that_finds_nothing_names_every_program_for_the_next_call() {
    use super::listing::search;
    let catalog = || {
        vec![
            listed("Microsoft.Paint_8wekyb3d8bbwe!App", "Paint", false),
            listed("Microsoft.WindowsNotepad_8wekyb3d8bbwe!App", "Notepad", true),
        ]
    };
    // The catalog names programs in the computer's display language, not in the user's words.
    let (found, notice) = search(catalog(), Some(" 画图 "));
    assert!(found.is_empty());
    let notice = notice.expect("a search that finds nothing says what there is");
    assert!(notice.contains("“画图”") && notice.contains("Notepad、Paint"), "{notice}");
    let (found, notice) = search(catalog(), Some("PAINT"));
    assert_eq!((names(found), notice), (vec!["Paint".to_owned()], None));
    let (found, notice) = search(catalog(), Some("windowsnotepad"));
    assert_eq!((names(found), notice), (vec!["Notepad".to_owned()], None), "the id counts too");
    let (found, notice) = search(catalog(), None);
    assert_eq!(
        (names(found), notice),
        (vec!["Notepad".to_owned(), "Paint".to_owned()], None),
        "the programs with a window first"
    );
}
