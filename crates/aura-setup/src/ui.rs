//! The setup and uninstall windows.
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result};
use aura_setup::{
    host::{Host, SystemHost},
    install::{self, AuraIsRunning, Kind, Options, Plan},
    payload,
    uninstall::{self, UninstallPlan},
};
use aura_windows_platform::installer::{Answer, Question, ask, choose_folder, run_with_progress};

const TITLE: &str = "Aura 安装程序";
const INSTALL: i32 = 100;
const RETRY: i32 = 101;
const DONE: i32 = 102;
const REMOVE: i32 = 103;
const CLOSE: i32 = 104;

fn message(instruction: &str, content: &str) {
    let _ = ask(
        &Question {
            title: TITLE,
            instruction,
            content,
            footer: None,
            buttons: &[(CLOSE, "关闭")],
            cancel: false,
            check_box: None,
        },
        None,
    );
}

/// Waits while Aura runs from `root`; `false` when the user gives up.
fn wait_until_closed(root: &Path) -> bool {
    loop {
        match SystemHost.running_in(root) {
            Ok(running) if running.is_empty() => return true,
            Ok(_) => {}
            Err(_) => return true,
        }
        let answer = ask(
            &Question {
                title: TITLE,
                instruction: "Aura 正在运行",
                content: "请先关闭 Aura，再点“重试”。",
                footer: None,
                buttons: &[(RETRY, "重试")],
                cancel: true,
                check_box: None,
            },
            None,
        );
        if !matches!(answer, Ok(Answer { button: Some(RETRY), .. })) {
            return false;
        }
    }
}

/// The license texts the release carries, written beside each other in a temporary folder and
/// opened in Notepad.
fn show_licenses(program: &Path) {
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

fn location_text(root: &Path) -> String {
    format!("安装位置：{}  <a href=\"change\">更改</a>", root.display())
}

fn heading(kind: &Kind, version: &str) -> String {
    match kind {
        Kind::Fresh => format!("安装 Aura {version}"),
        Kind::Upgrade { from } => format!("把 Aura 从 {from} 更新到 {version}"),
        Kind::Repair => format!("修复 Aura {version}"),
        Kind::Downgrade { from } => format!("把 Aura 从 {from} 换回 {version}"),
    }
}

pub fn install(program: &Path) -> Result<()> {
    let mut archive = payload::open_in_program(program)?;
    let release = match install::read_release(&mut archive) {
        Ok(release) => release,
        Err(error) => {
            message("无法安装", &format!("{error:#}"));
            return Ok(());
        }
    };
    let mut plan = install::plan(&SystemHost, &release, None)?;
    let has_licenses =
        payload::read_entry(&mut archive, &format!("versions/{}/LICENSE.txt", release.version))?
            .is_some();
    let instruction = heading(&plan.kind, &release.version);
    let fresh = plan.kind == Kind::Fresh;
    let mut chosen: Option<PathBuf> = None;
    let content = if fresh {
        location_text(&plan.root)
    } else {
        format!("安装位置：{}", plan.root.display())
    };
    let mut on_link = |href: &str, window: *mut std::ffi::c_void| -> Option<String> {
        if href == "license" {
            show_licenses(program);
            return None;
        }
        let folder = choose_folder(window, "选择安装 Aura 的文件夹").ok().flatten()?;
        let root = if folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("Aura")) {
            folder
        } else {
            folder.join("Aura")
        };
        let text = location_text(&root);
        chosen = Some(root);
        Some(text)
    };
    let footer = has_licenses.then_some("<a href=\"license\">许可协议与第三方声明</a>");
    let answer = ask(
        &Question {
            title: TITLE,
            instruction: &instruction,
            content: &content,
            footer,
            buttons: &[(INSTALL, if fresh { "安装" } else { "继续" })],
            cancel: true,
            check_box: fresh.then_some(("在桌面创建快捷方式", false)),
        },
        Some(&mut on_link),
    )?;
    if answer.button != Some(INSTALL) {
        return Ok(());
    }
    if fresh && let Some(root) = chosen {
        plan = match install::plan(&SystemHost, &release, Some(root)) {
            Ok(plan) => plan,
            Err(error) => {
                message("无法安装到这个位置", &format!("{error:#}"));
                return Ok(());
            }
        };
    }
    if !wait_until_closed(&plan.root) {
        return Ok(());
    }
    let options = Options { desktop_shortcut: fresh && answer.checked, while_running: false };
    let program_path = program.to_path_buf();
    let worker_plan = plan.clone();
    let result = run_with_progress(TITLE, &instruction, "正在复制文件…", move |progress| {
        let mut archive = payload::open_in_program(&program_path)?;
        let release = install::read_release(&mut archive)?;
        install::install(
            &SystemHost,
            &program_path,
            &mut archive,
            &release,
            &worker_plan,
            &options,
            &mut |done, total| progress.set(done, total),
        )
    })?;
    match result {
        Ok(()) => finished(&plan, &release.version),
        Err(error) if error.is::<AuraIsRunning>() => {
            message("安装没有完成", "Aura 在安装时启动了。请关闭 Aura 后重新运行安装程序。");
        }
        Err(error) => message("安装没有完成", &format!("{error:#}\n已安装的 Aura 没有改变。")),
    }
    Ok(())
}

fn finished(plan: &Plan, version: &str) {
    let instruction = match plan.kind {
        Kind::Fresh => format!("Aura {version} 已安装"),
        Kind::Repair => format!("Aura {version} 已修复"),
        Kind::Upgrade { .. } | Kind::Downgrade { .. } => {
            format!("Aura {version} 已就绪，下次启动 Aura 时使用")
        }
    };
    let answer = ask(
        &Question {
            title: TITLE,
            instruction: &instruction,
            content: "",
            footer: None,
            buttons: &[(DONE, "完成")],
            cancel: false,
            check_box: Some(("现在启动 Aura", true)),
        },
        None,
    );
    if answer.is_ok_and(|answer| answer.checked) {
        let _ = Command::new(plan.root.join(payload::LAUNCHER)).current_dir(&plan.root).spawn();
    }
}

fn size_text(bytes: u64) -> String {
    match bytes {
        0..1_048_576 => format!("{} KB", bytes.div_ceil(1024)),
        1_048_576..1_073_741_824 => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
        _ => format!("{:.1} GB", bytes as f64 / 1_073_741_824.0),
    }
}

/// Asks what to remove; `None` when the user cancels.
pub fn confirm_uninstall(plan: &UninstallPlan) -> Option<bool> {
    let instruction = format!("卸载 Aura {}", plan.version);
    let check = format!(
        "同时删除我的数据（会话、设置和记忆 {}，凭据 {} 条）",
        size_text(plan.data_bytes + plan.config_bytes),
        plan.credentials
    );
    let answer = ask(
        &Question {
            title: TITLE,
            instruction: &instruction,
            content: "会删除 Aura 程序和它的快捷方式。你的会话、设置、记忆和凭据默认保留，重新安装后可以接着用。工作区里的文件不会被删除。",
            footer: None,
            buttons: &[(REMOVE, "卸载")],
            cancel: true,
            check_box: Some((&check, false)),
        },
        None,
    )
    .ok()?;
    if answer.button != Some(REMOVE) {
        return None;
    }
    if !answer.checked {
        return Some(false);
    }
    let details = format!(
        "将删除：\n· 数据文件夹 {}（{}）\n· 设置文件夹 {}（{}）\n· 这个 Aura 保存的凭据 {} 条\n工作区里的文件不会被删除。",
        plan.data_root.display(),
        size_text(plan.data_bytes),
        plan.config_root.display(),
        size_text(plan.config_bytes),
        plan.credentials
    );
    let confirmed = ask(
        &Question {
            title: TITLE,
            instruction: "确认删除数据",
            content: &details,
            footer: None,
            buttons: &[(REMOVE, "删除并卸载")],
            cancel: true,
            check_box: None,
        },
        None,
    )
    .ok()?;
    (confirmed.button == Some(REMOVE)).then_some(true)
}

pub fn uninstall(root: &Path) -> Result<()> {
    let plan = match uninstall::plan_uninstall(&SystemHost, root) {
        Ok(plan) => plan,
        Err(error) => {
            message("无法卸载", &format!("{error:#}"));
            return Ok(());
        }
    };
    let Some(delete_data) = confirm_uninstall(&plan) else {
        return Ok(());
    };
    if !wait_until_closed(root) {
        return Ok(());
    }
    match uninstall::uninstall(&SystemHost, &plan, delete_data).context("uninstall") {
        Ok(_) => message(
            "Aura 已卸载",
            if delete_data { "程序和数据都已删除。" } else { "你的数据已保留。" },
        ),
        Err(error) => message("卸载没有完成", &format!("{error:#}")),
    }
    Ok(())
}
