//! The setup program's pages, as design B draws them: the location, Aura still running, the
//! work, the end; and the uninstaller's.
use std::path::Path;

use aura_old_data::{Kind as OldKind, OldData};
use aura_pixel_window::{
    Block, Button, Check, Face, Field, Footer, Frame, Kind as Look, Line, Link, Mark, Meter, Page,
    Pair, Snapshot, State, Step, Tone, format,
};
use time::UtcOffset;

use crate::{install::Kind, uninstall::UninstallPlan};

pub const TITLE: &str = "Aura 安装程序";

/// What the setup does to the installation, as its primary button and its steps call it.
fn verb(kind: &Kind) -> &'static str {
    match kind {
        Kind::Fresh => "安装",
        Kind::Upgrade { .. } => "更新",
        Kind::Repair => "修复",
        Kind::Downgrade { .. } => "换回",
        Kind::Relocate => "移动",
    }
}

pub fn heading(kind: &Kind, version: &str) -> String {
    match kind {
        Kind::Fresh => format!("安装 Aura {version}"),
        Kind::Upgrade { from } => format!("把 Aura 从 {from} 更新到 {version}"),
        Kind::Repair => format!("修复 Aura {version}"),
        Kind::Downgrade { from } => format!("把 Aura 从 {from} 换回 {version}"),
        Kind::Relocate => "更改 Aura 的位置".to_owned(),
    }
}

/// The column: the version, with the one it replaces, and the three steps, `at` the one shown.
fn frame(kind: &Kind, version: &str, at: usize) -> Frame {
    let shown = match kind {
        Kind::Upgrade { from } | Kind::Downgrade { from } => format!("{from} → {version}"),
        Kind::Fresh | Kind::Repair | Kind::Relocate => version.to_owned(),
    };
    let marks = ["位置", verb(kind), "完成"]
        .into_iter()
        .enumerate()
        .map(|(index, label)| Mark {
            label: label.to_owned(),
            state: match index.cmp(&at) {
                std::cmp::Ordering::Less => State::Done,
                std::cmp::Ordering::Equal => State::Now,
                std::cmp::Ordering::Greater => State::Todo,
            },
        })
        .collect();
    Frame::Setup { version: shown, marks }
}

fn page(
    frame: Frame,
    closable: bool,
    heading: impl Into<String>,
    blocks: Vec<Block>,
    footer: Footer,
) -> Page {
    Page { title: TITLE.to_owned(), frame, closable, heading: heading.into(), blocks, footer }
}

fn ends(buttons: Vec<Button>) -> Footer {
    Footer { link: None, start: Vec::new(), end: buttons }
}

/// Where the program and the data go, each with 更改. Only a new installation can choose a
/// desktop shortcut.
pub struct Location<'a> {
    pub kind: &'a Kind,
    pub version: &'a str,
    pub root: &'a Path,
    pub changed: bool,
    pub licenses: bool,
    pub shortcut: bool,
    pub error: Option<&'a str>,
    pub data: &'a Path,
    pub data_changed: bool,
    pub data_error: Option<&'a str>,
}

fn place(label: &str, folder: &Path, changed: bool, id: &'static str) -> Block {
    Block::Field(Field {
        label: label.to_owned(),
        show_label: true,
        value: folder.display().to_string(),
        changed,
        button: Some(Button::new(id, "更改", Look::Normal)),
        detail: Vec::new(),
    })
}

pub fn location(location: &Location<'_>) -> Page {
    let fresh = *location.kind == Kind::Fresh;
    let mut blocks = vec![place("安装位置", location.root, location.changed, "change")];
    if let Some(error) = location.error {
        blocks.push(Block::Text(Line::new(error, Tone::Danger, Face::Reading)));
    }
    blocks.push(place("数据位置", location.data, location.data_changed, "change-data"));
    if let Some(error) = location.data_error {
        blocks.push(Block::Text(Line::new(error, Tone::Danger, Face::Reading)));
    }
    if fresh {
        blocks.push(Block::Check(Check::new("shortcut", "在桌面创建快捷方式", location.shortcut)));
    }
    let mut go = Button::new("go", verb(location.kind), Look::Primary);
    // Moving without a new place has nothing to do.
    go.enabled = *location.kind != Kind::Relocate || location.changed || location.data_changed;
    let footer = Footer {
        link: location
            .licenses
            .then(|| Link { id: "licenses", label: "许可协议与第三方声明".to_owned() }),
        start: Vec::new(),
        end: vec![Button::new("cancel", "取消", Look::Normal), go],
    };
    page(
        frame(location.kind, location.version, 0),
        true,
        heading(location.kind, location.version),
        blocks,
        footer,
    )
}

/// Aura runs from the folder the setup has to change; `frame` is the page's column.
pub fn running(frame: Frame) -> Page {
    page(
        frame,
        true,
        "Aura 正在运行",
        vec![Block::Text(Line::new("关闭 Aura 后点“重试”", Tone::Ink, Face::Pixel))],
        ends(vec![
            Button::new("cancel", "取消", Look::Normal),
            Button::new("retry", "重试", Look::Primary),
        ]),
    )
}

pub fn running_frame(kind: &Kind, version: &str) -> Frame {
    frame(kind, version, 0)
}

/// The copy, then the check of what was copied; the bar follows the copy.
pub fn working(kind: &Kind, version: &str, progress: &Snapshot) -> Page {
    let copying = progress.total == 0 || progress.done < progress.total;
    let steps = vec![
        Step {
            label: "复制程序".to_owned(),
            state: if copying { State::Now } else { State::Done },
            value: (progress.total > 0).then(|| format::megabytes(progress.total)),
        },
        Step {
            label: "核对".to_owned(),
            state: if copying { State::Todo } else { State::Now },
            value: None,
        },
    ];
    let meter = Meter {
        fraction: progress.fraction(),
        left: if progress.total > 0 {
            format!(
                "已复制 {} / {}",
                format::megabytes(progress.done),
                format::megabytes(progress.total)
            )
        } else {
            String::new()
        },
        right: format::remaining(progress.done, progress.total, progress.elapsed)
            .unwrap_or_default(),
        words_above: false,
    };
    page(
        frame(kind, version, 1),
        false,
        format!("正在{}", verb(kind)),
        vec![Block::Steps(steps), Block::Meter(meter)],
        Footer::default(),
    )
}

/// Moving the installation: the copy, its check, the registrations, the old folder; `at` is the
/// step under way and the bar follows the copy and the check.
pub fn moving(kind: &Kind, version: &str, at: usize, progress: &Snapshot) -> Page {
    let steps = ["复制到新位置", "核对", "更新登记", "删除旧位置"]
        .into_iter()
        .enumerate()
        .map(|(index, label)| Step {
            label: label.to_owned(),
            state: match index.cmp(&at) {
                std::cmp::Ordering::Less => State::Done,
                std::cmp::Ordering::Equal => State::Now,
                std::cmp::Ordering::Greater => State::Todo,
            },
            value: (index == 0 && progress.total > 0).then(|| format::megabytes(progress.total)),
        })
        .collect();
    let meter = Meter {
        fraction: if at < 2 { progress.fraction() } else { None },
        left: String::new(),
        right: if at == 0 {
            format::remaining(progress.done, progress.total, progress.elapsed).unwrap_or_default()
        } else {
            String::new()
        },
        words_above: false,
    };
    page(
        frame(kind, version, 1),
        false,
        "正在移动 Aura",
        vec![Block::Steps(steps), Block::Meter(meter)],
        Footer::default(),
    )
}

/// The boxes of the old data the finished page lists, in order; the rest is left to 设置 → 存储.
pub const OLD_DATA: [&str; 6] = ["old-0", "old-1", "old-2", "old-3", "old-4", "old-5"];

/// What the finished page calls an item of old data.
pub fn old_data_name(item: &OldData) -> String {
    match item.kind {
        OldKind::EarlyLayout => "早期版本的数据".to_owned(),
        OldKind::Namespace => {
            let short = item.namespace_hash().and_then(|hash| hash.get(..8)).unwrap_or_default();
            format!("旧安装的数据 {short}")
        }
        OldKind::Unfinished => "没删完的旧数据".to_owned(),
    }
}

/// Which of the old data's boxes are ticked on `page`.
pub fn ticked(page: &Page, count: usize) -> Vec<bool> {
    OLD_DATA.iter().take(count).map(|id| page.checked(id)).collect()
}

/// The end; `notes` are what the user may still have to put right or should know, `old` the old
/// data to remove, each ticked or not, dated on a clock `offset` ahead of UTC.
pub fn finished(
    kind: &Kind,
    version: &str,
    notes: &[String],
    old: &[OldData],
    ticked: &[bool],
    start: bool,
    offset: UtcOffset,
) -> Page {
    let heading = match kind {
        Kind::Fresh => format!("Aura {version} 已安装"),
        Kind::Repair => format!("Aura {version} 已修复"),
        Kind::Upgrade { .. } | Kind::Downgrade { .. } => {
            format!("Aura {version} 已就绪，下次启动 Aura 时使用")
        }
        Kind::Relocate => "Aura 已移到新位置".to_owned(),
    };
    let mut blocks: Vec<Block> =
        notes.iter().map(|note| Block::Text(Line::new(note, Tone::Muted, Face::Reading))).collect();
    let old = &old[..old.len().min(OLD_DATA.len())];
    if !old.is_empty() {
        blocks.push(Block::Text(Line::new("旧数据", Tone::Ink, Face::Pixel)));
    }
    for (index, item) in old.iter().enumerate() {
        let label =
            [Some(old_data_name(item)), Some(format::size(item.bytes)), item.modified_day(offset)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
        let mut check =
            Check::new(OLD_DATA[index], label, ticked.get(index).copied().unwrap_or(true));
        check.notify = true;
        blocks.push(Block::Check(check));
    }
    blocks.push(Block::Check(Check::new("start", "现在启动 Aura", start)));
    let done = Button::new("done", "完成", Look::Primary);
    let footer = if old.is_empty() {
        ends(vec![done])
    } else {
        let mut remove = Button::new("remove-old", "删除所选", Look::Danger);
        remove.enabled = (0..old.len()).any(|index| ticked.get(index).copied().unwrap_or(true));
        Footer { link: None, start: vec![remove], end: vec![done] }
    };
    page(frame(kind, version, 2), true, heading, blocks, footer)
}

/// Removing the old data the user picked on the finished page.
pub fn removing_old(kind: &Kind, version: &str) -> Page {
    let meter =
        Meter { fraction: None, left: String::new(), right: String::new(), words_above: false };
    page(
        frame(kind, version, 2),
        false,
        "正在删除旧数据",
        vec![Block::Meter(meter)],
        Footer::default(),
    )
}

/// The work stopped; `detail` says why and `note` what it left as it was.
pub fn failed(kind: &Kind, version: &str, detail: &str, note: Option<&str>) -> Page {
    let mut blocks = vec![Block::Text(Line::new(detail, Tone::Ink, Face::Reading))];
    blocks.extend(note.map(|note| Block::Text(Line::new(note, Tone::Muted, Face::Pixel))));
    page(
        frame(kind, version, 1),
        true,
        "安装没有完成",
        blocks,
        ends(vec![Button::new("close", "关闭", Look::Primary)]),
    )
}

/// A page that only says something, before or without an installation to work on.
pub fn message(version: &str, heading: &str, detail: &str) -> Page {
    page(
        Frame::Setup { version: version.to_owned(), marks: Vec::new() },
        true,
        heading,
        vec![Block::Text(Line::new(detail, Tone::Ink, Face::Reading))],
        ends(vec![Button::new("close", "关闭", Look::Primary)]),
    )
}

fn uninstall_frame(plan: &UninstallPlan) -> Frame {
    Frame::Setup { version: plan.version.clone(), marks: Vec::new() }
}

/// What uninstalling removes: the program, and with the box ticked the data and settings too.
pub fn confirm_uninstall(plan: &UninstallPlan, delete_data: bool) -> Page {
    let pair = |key: &str, folder: &Path, bytes: u64| Pair {
        key: key.to_owned(),
        value: folder.display().to_string(),
        extra: Some(format::size(bytes)),
    };
    let mut check =
        Check::new("delete-data", "同时删除我的数据（会话、设置、记忆和凭据）", delete_data);
    check.notify = true;
    let remove = if delete_data {
        Button::new("remove", "删除并卸载", Look::Danger)
    } else {
        Button::new("remove", "卸载", Look::Primary)
    };
    page(
        uninstall_frame(plan),
        true,
        format!("卸载 Aura {}", plan.version),
        vec![
            Block::Pairs(vec![
                pair("程序", &plan.root, plan.program_bytes),
                pair("数据", &plan.data_root, plan.data_bytes),
                pair("设置", &plan.config_root, plan.config_bytes),
            ]),
            Block::Check(check),
            Block::Text(Line::new("工作区里的文件不会被删除", Tone::Muted, Face::Pixel)),
        ],
        ends(vec![Button::new("cancel", "取消", Look::Normal), remove]),
    )
}

pub fn uninstall_running(plan: &UninstallPlan) -> Page {
    running(uninstall_frame(plan))
}

pub fn uninstalling(plan: &UninstallPlan) -> Page {
    let meter =
        Meter { fraction: None, left: String::new(), right: String::new(), words_above: false };
    page(uninstall_frame(plan), false, "正在卸载", vec![Block::Meter(meter)], Footer::default())
}

pub fn uninstalled(plan: &UninstallPlan, delete_data: bool) -> Page {
    let detail =
        if delete_data { "程序和数据都已删除。" } else { "你的数据已保留。" };
    page(
        uninstall_frame(plan),
        true,
        "Aura 已卸载",
        vec![Block::Text(Line::new(detail, Tone::Ink, Face::Pixel))],
        ends(vec![Button::new("close", "关闭", Look::Primary)]),
    )
}

pub fn uninstall_failed(plan: &UninstallPlan, detail: &str) -> Page {
    page(
        uninstall_frame(plan),
        true,
        "卸载没有完成",
        vec![Block::Text(Line::new(detail, Tone::Ink, Face::Reading))],
        ends(vec![Button::new("close", "关闭", Look::Primary)]),
    )
}
