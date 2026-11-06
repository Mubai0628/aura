//! What the host reads and does in common programs, for the compatibility table: Notepad, Paint,
//! Calculator and Visual Studio Code. It opens them on the real desktop, so it is run by hand
//! (`--ignored --nocapture`) when no one is using the computer. It prints counts and outcomes
//! only, never what a window shows: Notepad may hold the user's own notes. A program that was
//! running already is only read: its windows are the user's.
//!
//! With `AURA_COMPAT_SHOTS` set to a folder, Calculator's screenshot with marks is saved there.
mod support;

use aura_desktop_control::desktop::RealDesktop;
use aura_desktop_control::host::Desktop;
use aura_desktop_domain::*;
use base64::Engine as _;
use std::collections::BTreeMap;
use support::*;

struct Program {
    label: &'static str,
    find: fn(&AppEntry) -> bool,
    /// What the test does in a program it opened itself.
    acts: fn(&mut RealDesktop, &AppEntry, &str, &[Element]) -> Vec<String>,
}

fn windows(desktop: &mut RealDesktop, apps: &[AppEntry], app: &AppEntry) -> Vec<WindowInfo> {
    match done(desktop.run(HostCommand::Windows { apps: apps.to_vec() })) {
        HostResult::Windows { windows } => windows
            .into_iter()
            .filter(|window| window.app_id.as_deref() == Some(app.id.as_str()))
            .collect(),
        other => panic!("{other:?}"),
    }
}

fn whole_read(
    desktop: &mut RealDesktop,
    app: &AppEntry,
) -> Result<(WindowInfo, Vec<Element>, bool), HostError> {
    match read(desktop, app, None, 2_000, 32, 10_000) {
        HostOutcome::Done(HostResult::Elements { window, elements, complete }) => {
            Ok((window, elements, complete))
        }
        HostOutcome::Done(other) => panic!("{other:?}"),
        HostOutcome::Failed(error) => Err(error),
    }
}

fn by_automation_id<'a>(elements: &'a [Element], id: &str) -> Option<&'a Element> {
    elements.iter().find(|element| element.key.automation_id == id)
}

fn outcome(name: &str, result: &HostOutcome) -> String {
    match result {
        HostOutcome::Done(_) => format!("{name} 成功"),
        HostOutcome::Failed(error) => format!("{name} 失败（{:?}）", error.code),
    }
}

fn arrange(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    window: &str,
    action: WindowAction,
) -> HostOutcome {
    desktop.run(HostCommand::Window { app: app.clone(), window: Some(window.into()), action })
}

fn calculator(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    window: &str,
    elements: &[Element],
) -> Vec<String> {
    let mut said = Vec::new();
    let keys = ["num1Button", "plusButton", "num2Button", "equalButton"];
    let steps: Option<Vec<ActStep>> = keys
        .iter()
        .map(|id| {
            by_automation_id(elements, id)
                .map(|element| ActStep::Click { element: element.key.clone() })
        })
        .collect();
    let Some(steps) = steps else {
        said.push("找不到数字键".into());
        return said;
    };
    let clicked = act(desktop, app, window, steps);
    said.push(outcome("按元素点 1 + 2 =", &clicked));
    if let Ok((_, after, _)) = whole_read(desktop, app) {
        let shows_three = by_automation_id(&after, "CalculatorResults")
            .is_some_and(|result| result.key.name.contains('3'));
        said.push(format!("结果显示 3：{}", if shows_three { "是" } else { "否" }));
        if let Some(folder) = std::env::var_os("AURA_COMPAT_SHOTS") {
            let marks: Vec<Mark> = after
                .iter()
                .enumerate()
                .filter(|(_, element)| !element.abilities.is_empty() && !element.rect.is_empty())
                .map(|(at, element)| Mark { number: at as u32 + 1, rect: element.rect })
                .collect();
            let shot = desktop.run(HostCommand::Screenshot {
                show: vec![app.clone()],
                paint: true,
                max_long_edge: 1_568,
                region: None,
                marks: marks.clone(),
            });
            if let HostOutcome::Done(HostResult::Screenshot { png, .. }) = &shot {
                let bytes = base64::engine::general_purpose::STANDARD.decode(png).expect("base64");
                std::fs::write(std::path::Path::new(&folder).join("calculator-marks.png"), bytes)
                    .expect("saved");
            }
            said.push(format!("{}（{} 个编号框）", outcome("带编号框截图", &shot), marks.len()));
        }
    }
    said
}

fn paint(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    window: &str,
    elements: &[Element],
) -> Vec<String> {
    let pencil =
        elements.iter().find(|element| {
            element.abilities.iter().any(|ability| {
                matches!(ability, Ability::Click | Ability::Select | Ability::Toggle)
            }) && (element.key.name.contains("铅笔") || element.key.name.contains("Pencil"))
        });
    match pencil {
        Some(pencil) => {
            let step = ActStep::Click { element: pencil.key.clone() };
            vec![outcome("按元素选铅笔", &act(desktop, app, window, vec![step]))]
        }
        None => vec!["找不到铅笔".into()],
    }
}

fn notepad(
    desktop: &mut RealDesktop,
    app: &AppEntry,
    window: &str,
    elements: &[Element],
) -> Vec<String> {
    // Nothing that changes text: the window may show the user's own tabs.
    let menu = elements.iter().find(|element| {
        element.role == Role::MenuItem && element.abilities.contains(&Ability::ExpandCollapse)
    });
    match menu {
        Some(menu) => {
            let steps = vec![
                ActStep::Expand { element: menu.key.clone() },
                ActStep::Collapse { element: menu.key.clone() },
            ];
            vec![outcome("按元素展开再收起菜单", &act(desktop, app, window, steps))]
        }
        None => vec!["没有能展开的菜单项".into()],
    }
}

fn nothing(_: &mut RealDesktop, _: &AppEntry, _: &str, _: &[Element]) -> Vec<String> {
    vec!["只读取（Chromium 的无障碍树打开后要到程序退出才关）".into()]
}

const PROGRAMS: [Program; 4] = [
    Program {
        label: "记事本",
        find: |app| app.id.starts_with("Microsoft.WindowsNotepad_"),
        acts: notepad,
    },
    Program { label: "画图", find: |app| app.id.starts_with("Microsoft.Paint_"), acts: paint },
    Program {
        label: "计算器",
        find: |app| app.id.starts_with("Microsoft.WindowsCalculator_"),
        acts: calculator,
    },
    Program {
        label: "VS Code",
        find: |app| {
            app.name.eq_ignore_ascii_case("Visual Studio Code")
                || app
                    .path
                    .as_deref()
                    .is_some_and(|path| path.to_ascii_lowercase().ends_with("\\code.exe"))
        },
        acts: nothing,
    },
];

#[test]
#[ignore = "opens Notepad, Paint, Calculator and VS Code: run by hand when no one is using the computer"]
fn the_compatibility_table() {
    let mut desktop = desktop();
    let apps = catalog(&mut desktop, Vec::new());
    println!("| 程序 | 打开 | 元素 | 读完 | 能操作的 | 主要角色 | 操作 |");
    println!("|---|---|---|---|---|---|---|");
    for program in PROGRAMS {
        let Some(app) = apps.iter().find(|app| (program.find)(app)).cloned() else {
            println!("| {} | 没装 | | | | | |", program.label);
            continue;
        };
        let before = windows(&mut desktop, &apps, &app);
        let opened = before.is_empty();
        if opened {
            let launched =
                desktop.run(HostCommand::Launch { app: app.clone(), timeout_ms: 20_000 });
            if let HostOutcome::Failed(error) = launched {
                println!("| {} | 打不开（{:?}） | | | | | |", program.label, error.code);
                continue;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        let (window, elements, complete) = match whole_read(&mut desktop, &app) {
            Ok(read) => read,
            Err(error) => {
                println!(
                    "| {} | {} | 读不到（{:?}） | | | | |",
                    program.label,
                    if opened { "打开" } else { "已在运行" },
                    error.code
                );
                continue;
            }
        };
        let mut roles: BTreeMap<String, usize> = BTreeMap::new();
        for element in &elements {
            *roles.entry(format!("{:?}", element.role)).or_default() += 1;
        }
        let mut common: Vec<(String, usize)> = roles.into_iter().collect();
        common.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
        let common: Vec<String> =
            common.iter().take(4).map(|(role, count)| format!("{role} {count}")).collect();
        let actionable = elements.iter().filter(|element| !element.abilities.is_empty()).count();
        let mut said = if opened {
            (program.acts)(&mut desktop, &app, &window.window, &elements)
        } else {
            vec!["已在运行：只读取，不操作用户的窗口".into()]
        };
        if opened {
            for action in [WindowAction::Minimize, WindowAction::Restore] {
                said.push(outcome(
                    &format!("{action:?}"),
                    &arrange(&mut desktop, &app, &window.window, action),
                ));
            }
            let closed = arrange(&mut desktop, &app, &window.window, WindowAction::Close);
            let still_open = matches!(&closed, HostOutcome::Done(HostResult::Window { window: target, windows, .. })
                if windows.iter().any(|one| &one.window == target));
            said.push(if still_open {
                "关闭后窗口还开着".into()
            } else {
                outcome("关闭", &closed)
            });
        }
        println!(
            "| {} | {} | {} | {} | {} | {} | {} |",
            program.label,
            if opened { "打开" } else { "已在运行" },
            elements.len(),
            if complete { "是" } else { "否" },
            actionable,
            common.join("、"),
            said.join("；")
        );
    }
}
