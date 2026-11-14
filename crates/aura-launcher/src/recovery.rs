//! The launcher's windows: what keeps Aura from starting and what the user can do about it, and
//! the work that runs before Aura opens. They are the setup program's pixel windows with the
//! launcher's narrow column.
use std::io;

use aura_pixel_window::{
    Action, Block, Button, Face, Footer, Frame, Icon, Kind, Line, Meter, Page, PixelWindow,
    Snapshot, Tone, format,
};

pub const TITLE: &str = "Aura";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryAction {
    Restart,
    SafeMode,
    ExportDiagnostics,
    RollBack,
    /// Starts a newer version whose data copy could not be taken.
    StartWithoutBackup,
    /// Looks for the installation's data again.
    Retry,
    /// Points Aura at the folder the data is in now.
    ChooseLocation,
    Exit,
}

impl RecoveryAction {
    const ALL: [Self; 8] = [
        Self::Restart,
        Self::SafeMode,
        Self::ExportDiagnostics,
        Self::RollBack,
        Self::StartWithoutBackup,
        Self::Retry,
        Self::ChooseLocation,
        Self::Exit,
    ];

    const fn id(self) -> &'static str {
        match self {
            Self::Restart => "restart",
            Self::SafeMode => "safe-mode",
            Self::ExportDiagnostics => "export",
            Self::RollBack => "roll-back",
            Self::StartWithoutBackup => "start-without-backup",
            Self::Retry => "retry",
            Self::ChooseLocation => "choose-location",
            Self::Exit => "exit",
        }
    }

    fn label(self, previous_version: Option<&str>) -> String {
        match self {
            Self::Restart => "重新启动".to_owned(),
            Self::SafeMode => "安全模式".to_owned(),
            Self::ExportDiagnostics => "导出诊断".to_owned(),
            Self::RollBack => format!("回到 {}", previous_version.unwrap_or_default()),
            Self::StartWithoutBackup => "不备份，启动新版本".to_owned(),
            Self::Retry => "重试".to_owned(),
            Self::ChooseLocation => "重新选择位置".to_owned(),
            Self::Exit => "退出".to_owned(),
        }
    }
}

/// What a recovery window says and offers. The first action named besides `Exit` is the one
/// Enter takes, unless it gives up the data's copy; `RollBack` is offered only with a
/// `previous_version`.
pub struct Recovery<'a> {
    pub heading: &'a str,
    pub blocks: Vec<Block>,
    pub actions: &'a [RecoveryAction],
    pub previous_version: Option<&'a str>,
}

/// A line of the launcher's own words, in the pixel face.
pub fn words(text: impl Into<String>, tone: Tone) -> Block {
    Block::Text(Line::new(text, tone, Face::Pixel))
}

/// A line that quotes what the system or the records said, in the reading face.
pub fn quoted(text: impl Into<String>) -> Block {
    Block::Text(Line::new(text, Tone::Muted, Face::Reading))
}

/// No copy of the data could be taken before `version` upgrades it; `cause` says why.
pub fn backup_failed<'a>(version: &str, heading: &'a str, cause: &str) -> Recovery<'a> {
    Recovery {
        heading,
        blocks: vec![
            words(format!("新版本 {version} 第一次启动会升级数据，启动前要先备份"), Tone::Ink),
            quoted(cause),
            words("不备份就启动，以后回到上一版本时不能恢复更新前的数据", Tone::Muted),
        ],
        actions: &[RecoveryAction::Exit, RecoveryAction::StartWithoutBackup],
        previous_version: None,
    }
}

/// The active version's files do not check out; `detail` says how.
pub fn startup_failed<'a>(active: &str, detail: &str, previous: Option<&'a str>) -> Recovery<'a> {
    let repair = if previous.is_some() {
        "也可以重新运行 Aura 安装程序修复"
    } else {
        "重新运行 Aura 安装程序可以修复"
    };
    Recovery {
        heading: "Aura 无法启动",
        blocks: vec![
            words(format!("当前版本 {active} 的程序文件不完整或被改动"), Tone::Ink),
            quoted(detail),
            words(repair, Tone::Muted),
        ],
        actions: &[RecoveryAction::RollBack, RecoveryAction::Exit],
        previous_version: previous,
    }
}

/// A component keeps exiting, so the launcher stopped starting it.
pub fn crash_loop<'a>(
    component: &str,
    exit_code: u32,
    active: &str,
    previous: Option<&'a str>,
) -> Recovery<'a> {
    Recovery {
        heading: "Aura 已停止以避免重复崩溃",
        blocks: vec![
            words(format!("组件 {component} 在 10 分钟内反复退出"), Tone::Ink),
            words(format!("退出代码 {exit_code} · 当前版本 {active}"), Tone::Muted),
        ],
        actions: &[
            RecoveryAction::Restart,
            RecoveryAction::SafeMode,
            RecoveryAction::RollBack,
            RecoveryAction::ExportDiagnostics,
            RecoveryAction::Exit,
        ],
        previous_version: previous,
    }
}

/// The launcher itself failed; `detail` says how.
pub fn fatal(detail: &str) -> Recovery<'static> {
    Recovery {
        heading: "Aura 无法启动",
        blocks: vec![quoted(detail), words("请重新运行 Aura 安装程序修复", Tone::Muted)],
        actions: &[RecoveryAction::Exit],
        previous_version: None,
    }
}

pub fn recovery_page(recovery: &Recovery<'_>) -> Page {
    let offered: Vec<RecoveryAction> = recovery
        .actions
        .iter()
        .copied()
        .filter(|action| *action != RecoveryAction::RollBack || recovery.previous_version.is_some())
        .collect();
    let others: Vec<RecoveryAction> =
        offered.iter().copied().filter(|action| *action != RecoveryAction::Exit).collect();
    let primary =
        others.iter().copied().find(|action| *action != RecoveryAction::StartWithoutBackup);
    let button = |action: RecoveryAction| {
        let kind = match action {
            RecoveryAction::StartWithoutBackup => Kind::Danger,
            _ if Some(action) == primary => Kind::Primary,
            _ => Kind::Normal,
        };
        Button::new(action.id(), action.label(recovery.previous_version), kind)
    };
    let exits = offered.contains(&RecoveryAction::Exit);
    // The choice Enter takes stands at the right edge, the others before it in turn; leaving
    // stands apart at the left, or alone at the right when it is the only choice.
    let (start, end) = if others.is_empty() {
        (
            Vec::new(),
            exits.then(|| Button::new("exit", "退出", Kind::Primary)).into_iter().collect(),
        )
    } else {
        let start = exits.then(|| button(RecoveryAction::Exit)).into_iter().collect();
        (start, others.iter().rev().copied().map(button).collect())
    };
    Page {
        title: TITLE.to_owned(),
        frame: Frame::Launcher { icon: Icon::Warning, fish: false },
        closable: true,
        heading: recovery.heading.to_owned(),
        blocks: recovery.blocks.clone(),
        footer: Footer { link: None, start, end },
    }
}

/// Shows the recovery window until the user picks; closing it is `Exit`.
pub fn ask(recovery: &Recovery<'_>) -> io::Result<RecoveryAction> {
    let mut page = recovery_page(recovery);
    let mut window = PixelWindow::open(&page).map_err(io::Error::other)?;
    let action = window.show(&mut page).map_err(io::Error::other)?;
    Ok(match action {
        Action::Button(id) => RecoveryAction::ALL
            .into_iter()
            .find(|action| action.id() == id)
            .unwrap_or(RecoveryAction::Exit),
        Action::Link(_) | Action::Toggled(_) | Action::Close => RecoveryAction::Exit,
    })
}

/// The window for work that runs before Aura opens: its stage and how far it got, or a copy.
pub fn progress_page(heading: &str, progress: &Snapshot) -> Page {
    // Until the work names a stage or a size there is nothing to measure against.
    let waiting = progress.stage.is_none() && progress.total == 0;
    let fraction = if waiting { None } else { progress.fraction() };
    let (left, right, words_above) = match &progress.stage {
        Some(stage) if progress.unmeasured => (stage.clone(), String::new(), true),
        Some(stage) => {
            let percent = (fraction.unwrap_or(0.0) * 100.0).floor() as u64;
            (stage.clone(), format!("{percent}%"), true)
        }
        None if waiting => (String::new(), String::new(), false),
        None => (
            format!(
                "已复制 {} / {}",
                format::megabytes(progress.done),
                format::megabytes(progress.total)
            ),
            format::remaining(progress.done, progress.total, progress.elapsed).unwrap_or_default(),
            false,
        ),
    };
    Page {
        title: TITLE.to_owned(),
        frame: Frame::Launcher { icon: Icon::Logo, fish: true },
        closable: false,
        heading: heading.to_owned(),
        blocks: vec![Block::Meter(Meter { fraction, left, right, words_above })],
        footer: Footer::default(),
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
