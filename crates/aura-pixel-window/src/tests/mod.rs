//! Pages for the tests, and a lock so tests that open windows run one at a time (they find
//! windows by enumerating this process's).
mod accessibility;
mod art;
mod audit;
mod format;
mod keyboard;
mod on_screen;
mod progress;
mod scale;

use std::sync::{Mutex, MutexGuard};

use crate::page::{
    Block, Button, Check, Face, Field, Footer, Frame, Icon, Kind, Line, Link, Mark, Meter, Page,
    Pair, State, Step, Tone,
};

static WINDOWS: Mutex<()> = Mutex::new(());

/// Held while a test has a window open; windows are never shown.
pub(crate) fn one_window_at_a_time() -> MutexGuard<'static, ()> {
    crate::testing::hide_windows();
    WINDOWS.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) fn install_page() -> Page {
    Page {
        title: "Aura 安装程序".to_owned(),
        frame: Frame::Setup {
            version: "0.3.0".to_owned(),
            marks: vec![
                Mark { label: "位置".to_owned(), state: State::Now },
                Mark { label: "安装".to_owned(), state: State::Todo },
                Mark { label: "完成".to_owned(), state: State::Todo },
            ],
        },
        closable: true,
        heading: "安装 Aura 0.3.0".to_owned(),
        blocks: vec![
            Block::Field(Field {
                label: "安装位置".to_owned(),
                show_label: true,
                value: r"C:\Users\14153\AppData\Local\Programs\Aura".to_owned(),
                changed: false,
                button: Some(Button::new("change", "更改", Kind::Normal)),
                detail: Vec::new(),
            }),
            Block::Check(Check::new("shortcut", "在桌面创建快捷方式", false)),
        ],
        footer: Footer {
            link: Some(Link { id: "licenses", label: "许可协议与第三方声明".to_owned() }),
            start: Vec::new(),
            end: vec![
                Button::new("cancel", "取消", Kind::Normal),
                Button::new("install", "安装", Kind::Primary),
            ],
        },
    }
}

/// Every kind of block and button state on one page.
pub(crate) fn every_block_page() -> Page {
    let mut disabled = Button::new("later", "稍后", Kind::Normal);
    disabled.enabled = false;
    Page {
        title: "Aura 安装程序".to_owned(),
        frame: Frame::Setup {
            version: "0.2.0 → 0.3.0".to_owned(),
            marks: vec![
                Mark { label: "位置".to_owned(), state: State::Done },
                Mark { label: "更新".to_owned(), state: State::Now },
                Mark { label: "完成".to_owned(), state: State::Todo },
            ],
        },
        closable: false,
        heading: "把 Aura 从 0.2.0 更新到 0.3.0".to_owned(),
        blocks: vec![
            Block::Field(Field {
                label: "数据位置".to_owned(),
                show_label: true,
                value: r"D:\AuraData".to_owned(),
                changed: true,
                button: Some(Button::new("change-data", "更改", Kind::Normal)),
                detail: vec![
                    ("从".to_owned(), Face::Pixel),
                    (r"C:\Users\14153\AppData\Local\Aura".to_owned(), Face::Reading),
                    ("搬过来 · 8.61 GB".to_owned(), Face::Pixel),
                ],
            }),
            Block::Steps(vec![
                Step { label: "设好权限".to_owned(), state: State::Done, value: None },
                Step {
                    label: "复制程序".to_owned(),
                    state: State::Done,
                    value: Some("1,664 MB".to_owned()),
                },
                Step {
                    label: "搬数据".to_owned(),
                    state: State::Now,
                    value: Some("62%".to_owned()),
                },
                Step { label: "核对".to_owned(), state: State::Todo, value: None },
            ]),
            Block::Meter(Meter {
                fraction: Some(0.62),
                left: "已复制 5,338 MB / 8,610 MB".to_owned(),
                right: "剩余约 40 秒".to_owned(),
                words_above: false,
            }),
            Block::Pairs(vec![
                Pair {
                    key: "程序".to_owned(),
                    value: r"D:\Aura".to_owned(),
                    extra: Some("1.66 GB".to_owned()),
                },
                Pair {
                    key: "数据".to_owned(),
                    value: r"D:\AuraData".to_owned(),
                    extra: Some("8.61 GB".to_owned()),
                },
            ]),
            Block::Check(Check::new("start", "现在启动 Aura", true)),
            Block::Text(Line::new("工作区里的文件不会被删除", Tone::Muted, Face::Pixel)),
            Block::Text(Line::new("aura-core.exe 与发布清单不一致", Tone::Muted, Face::Reading)),
            Block::Text(Line::new("安装没有完成", Tone::Danger, Face::Pixel)),
        ],
        footer: Footer {
            link: None,
            start: vec![Button::new("delete", "删除所选（875 MB）", Kind::Danger)],
            end: vec![disabled, Button::new("done", "完成", Kind::Primary)],
        },
    }
}

/// A launcher window: the warning mark, a location and the recovery buttons.
pub(crate) fn launcher_page() -> Page {
    Page {
        title: "Aura".to_owned(),
        frame: Frame::Launcher { icon: Icon::Warning, fish: false },
        closable: true,
        heading: "找不到数据位置".to_owned(),
        blocks: vec![
            Block::Field(Field {
                label: "数据位置".to_owned(),
                show_label: false,
                value: r"D:\AuraData".to_owned(),
                changed: false,
                button: None,
                detail: Vec::new(),
            }),
            Block::Text(Line::new(
                "数据文件夹不在：系统找不到指定的路径。",
                Tone::Ink,
                Face::Pixel,
            )),
        ],
        footer: Footer {
            link: None,
            start: vec![Button::new("exit", "退出", Kind::Normal)],
            end: vec![Button::new("retry", "重试", Kind::Primary)],
        },
    }
}

/// A launcher window with work running: the mark, the shark and a bar without a position.
pub(crate) fn launcher_progress_page() -> Page {
    Page {
        title: "Aura".to_owned(),
        frame: Frame::Launcher { icon: Icon::Logo, fish: true },
        closable: false,
        heading: "正在整理数据".to_owned(),
        blocks: vec![Block::Meter(Meter {
            fraction: None,
            left: "整理数据库文件".to_owned(),
            right: String::new(),
            words_above: true,
        })],
        footer: Footer::default(),
    }
}
