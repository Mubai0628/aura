//! Which programs the catalog has and which program a window belongs to: the rules alone, with
//! made-up Start menu items.
use aura_desktop_control::catalog::{StartMenuItem, WindowFacts, catalog, owner};
use aura_desktop_domain::{AppEntry, AppKind, ManualApp};

fn item(parsing_name: &str, name: &str, target: Option<&str>) -> StartMenuItem {
    StartMenuItem {
        parsing_name: parsing_name.into(),
        name: name.into(),
        target: target.map(Into::into),
    }
}

const NOTEPAD: &str = "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App";
const WEIXIN: &str = r"{6D809377-6AF0-444B-8957-A3773F02200E}\Tencent\Weixin\Weixin.exe";
const WEIXIN_PATH: &str = r"C:\Program Files\Tencent\Weixin\Weixin.exe";

fn start_menu() -> Vec<StartMenuItem> {
    vec![
        item(NOTEPAD, "记事本", None),
        item(WEIXIN, "微信", Some(WEIXIN_PATH)),
        item(
            "Microsoft.VisualStudioCode",
            "Visual Studio Code",
            Some(r"C:\Users\u\AppData\Local\Programs\Microsoft VS Code\Code.exe"),
        ),
        item(
            r"{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\Foo\Readme.chm",
            "Foo 帮助",
            Some(r"C:\Program Files (x86)\Foo\Readme.chm"),
        ),
        item("https://example.com/", "网站", None),
        item("https://example.com/get", "下载器", Some(r"C:\Program Files\Foo\get.exe")),
        item(r"{7C5A40EF-A0FB-4BFC-874A-C0F2E0B9FA8E}\Foo\uninstall.txt", "说明", None),
        item("com.squirrel.slack.slack", "Slack", None),
        item(NOTEPAD, "记事本（重复）", None),
        item("Some.App", "  ", None),
    ]
}

#[test]
fn the_catalog_has_the_programs_of_all_apps_once_each_and_nothing_else() {
    let apps = catalog(start_menu(), &[]);
    let listed: Vec<(&str, &str, AppKind)> =
        apps.iter().map(|app| (app.id.as_str(), app.name.as_str(), app.kind)).collect();
    assert_eq!(
        listed,
        [
            (NOTEPAD, "记事本", AppKind::Packaged),
            (WEIXIN, "微信", AppKind::Desktop),
            ("Microsoft.VisualStudioCode", "Visual Studio Code", AppKind::Desktop),
            ("com.squirrel.slack.slack", "Slack", AppKind::Desktop),
        ]
    );
    assert_eq!(apps[1].path.as_deref(), Some(WEIXIN_PATH));
    assert_eq!(apps[0].path, None);
}

#[test]
fn a_program_the_user_added_follows_unless_all_apps_has_it_already() {
    let manual = [
        ManualApp { name: "绿色工具".into(), path: r"D:\Tools\green.exe".into() },
        ManualApp { name: "又一个微信".into(), path: WEIXIN_PATH.to_lowercase() },
        ManualApp { name: "".into(), path: r"D:\Tools\other.exe".into() },
        ManualApp { name: "相对路径".into(), path: r"Tools\x.exe".into() },
        ManualApp { name: "不是程序".into(), path: r"D:\Tools\notes.txt".into() },
    ];
    let apps = catalog(start_menu(), &manual);
    let added: Vec<&AppEntry> = apps.iter().filter(|app| app.kind == AppKind::Manual).collect();
    assert_eq!(added.len(), 2, "{apps:?}");
    assert_eq!((added[0].id.as_str(), added[0].name.as_str()), (r"D:\Tools\green.exe", "绿色工具"));
    assert_eq!(added[0].path.as_deref(), Some(r"D:\Tools\green.exe"));
    assert_eq!(added[1].name, "other.exe", "a nameless program is named by its file");
}

#[test]
fn a_window_belongs_to_its_aumid_then_its_program_file_then_a_unique_program_name() {
    let apps = catalog(
        start_menu(),
        &[ManualApp { name: "另一个 Code".into(), path: r"D:\Portable\Code.exe".into() }],
    );
    let of = |facts: WindowFacts<'_>| owner(&apps, &facts).map(|app| app.name.as_str());
    let notepad_process = r"C:\Program Files\WindowsApps\Microsoft.WindowsNotepad_11.2508.38.0_x64__8wekyb3d8bbwe\Notepad\Notepad.exe";
    assert_eq!(
        of(WindowFacts {
            aumid: Some(&NOTEPAD.to_lowercase()),
            process_path: Some(notepad_process),
            process_name: "Notepad.exe"
        }),
        Some("记事本")
    );
    assert_eq!(
        of(WindowFacts {
            aumid: None,
            process_path: Some(&WEIXIN_PATH.to_uppercase()),
            process_name: "Weixin.exe"
        }),
        Some("微信")
    );
    assert_eq!(
        of(WindowFacts {
            aumid: Some("Tencent.Weixin.Other"),
            process_path: Some(r"E:\wx\Weixin.exe"),
            process_name: "weixin.exe"
        }),
        Some("微信"),
        "an AUMID no program has falls back to the program file, then to its name"
    );
    assert_eq!(
        of(WindowFacts {
            aumid: None,
            process_path: Some(r"E:\elsewhere\Code.exe"),
            process_name: "Code.exe"
        }),
        None,
        "two programs are named Code.exe: the name alone does not say which"
    );
    assert_eq!(
        of(WindowFacts {
            aumid: Some("com.squirrel.slack.slack"),
            process_path: Some(r"C:\Users\u\AppData\Local\slack\app-4.41\slack.exe"),
            process_name: "slack.exe"
        }),
        Some("Slack")
    );
    assert_eq!(of(WindowFacts { aumid: None, process_path: None, process_name: "" }), None);
}
