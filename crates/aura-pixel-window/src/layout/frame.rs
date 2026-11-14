//! The window around the page: its edge and shadow, the title bar, the close button, and the
//! column of art on the left with the dithered sea at its foot.
use super::{Builder, Node, Role, Target, controls};
use crate::{
    art,
    page::{Face, Frame, Icon, Page, State},
    scene::{Item, Rect},
    theme::{
        ACCENT, FAINT, INK, LINE, MUTED, PANE, QUIET, SEA, SEA_DEEP, SHADOW_ALPHA, SHADOW_OFFSET,
        SURFACE_HOVER, SURFACE_SELECTED, WARNING, WHITE,
    },
};

pub(super) const BAR_HEIGHT: i32 = 32;
/// The window's edge, the title bar and the line under it.
pub(super) const MAIN_TOP: i32 = 1 + BAR_HEIGHT + 1;
const SETUP_WIDTH: i32 = 760;
const LAUNCHER_WIDTH: i32 = 480;

pub(super) fn column_width(frame: &Frame) -> i32 {
    match frame {
        Frame::Setup { .. } => 232,
        Frame::Launcher { .. } => 96,
    }
}

pub(super) fn min_main(frame: &Frame) -> i32 {
    match frame {
        Frame::Setup { .. } => 400,
        Frame::Launcher { .. } => 200,
    }
}

/// The setup program's width is fixed; a launcher window widens until its buttons fit.
pub(super) fn window_width(builder: &mut Builder, page: &Page) -> i32 {
    match page.frame {
        Frame::Setup { .. } => SETUP_WIDTH,
        Frame::Launcher { .. } => {
            let needed = 2 + column_width(&page.frame) + controls::footer_width(builder, page);
            needed.max(LAUNCHER_WIDTH)
        }
    }
}

/// The shadow, the white window with its dark edge, and the title bar.
pub(super) fn shell(builder: &mut Builder, page: &Page, width: i32, height: i32) {
    builder.scene.items.push(Item::Fill {
        rect: Rect::new(SHADOW_OFFSET, SHADOW_OFFSET, width, height),
        colour: INK,
        alpha: SHADOW_ALPHA,
    });
    builder.scene.fill(Rect::new(0, 0, width, height), WHITE);
    builder.scene.frame(Rect::new(0, 0, width, height), INK);
    builder.scene.fill(Rect::new(1, 1 + BAR_HEIGHT, width - 2, 1), LINE);
    let room = width - 2 - 12 - if page.closable { BAR_HEIGHT + 8 } else { 12 };
    builder.words(&page.title, (Face::Pixel, 12, 16), (INK, WHITE), 13, 1 + 8, Some(room), false);
}

pub(super) fn close_button(builder: &mut Builder, page: &Page, width: i32) {
    if !page.closable {
        return;
    }
    let rect = Rect::new(width - 1 - BAR_HEIGHT, 1, BAR_HEIGHT, BAR_HEIGHT);
    let node = builder.node(Node::new(Role::Button, "关闭", rect, "close"));
    let index = builder.control(Target::Close, rect, true, false, node);
    let (hover, pressed, _) = builder.state(index);
    if pressed {
        builder.scene.fill(rect, SURFACE_SELECTED);
    } else if hover {
        builder.scene.fill(rect, SURFACE_HOVER);
    }
    builder.scene.art(&art::CLOSE, rect.x + 8, rect.y + 8, 1, INK, true);
}

/// The column: its mark, for the setup program its version and steps, and the sea.
pub(super) fn column(builder: &mut Builder, page: &Page, area: Rect) {
    builder.scene.fill(Rect::new(area.x, area.y, area.width - 1, area.height), PANE);
    builder.scene.fill(Rect::new(area.right() - 1, area.y, 1, area.height), LINE);
    let x = area.x + 24;
    let top = area.y + 24;
    match &page.frame {
        Frame::Setup { version, marks } => {
            builder.scene.art(&art::LOGO, x, top, 2, INK, true);
            let brand = top + 64 + 12;
            let room = area.width - 1 - 48;
            let name =
                builder.words("Aura", (Face::Pixel, 24, 30), (INK, PANE), x, brand, None, false);
            // The version sits on the name's baseline.
            let baseline = brand + builder.measure.baseline(Face::Pixel, 24, 30);
            let small = baseline - builder.measure.baseline(Face::Pixel, 12, 16);
            builder.words(
                version,
                (Face::Pixel, 12, 16),
                (MUTED, PANE),
                x + name + 8,
                small,
                Some(room - name - 8),
                false,
            );
            let mut y = brand + 30 + 32;
            for mark in marks {
                marker(builder, x, y + 2, mark.state);
                let colour = match mark.state {
                    State::Now => INK,
                    State::Done => MUTED,
                    State::Todo => QUIET,
                };
                builder.words(
                    &mark.label,
                    (Face::Pixel, 12, 16),
                    (colour, PANE),
                    x + 22,
                    y,
                    Some(room - 22),
                    false,
                );
                y += 16 + 12;
            }
            sea(builder, area, Some(64));
        }
        Frame::Launcher { icon, fish } => {
            match icon {
                Icon::Logo => builder.scene.art(&art::LOGO, x, top, 1, INK, true),
                Icon::Warning => builder.scene.art(&art::WARNING, x, top, 2, WARNING, true),
            }
            sea(builder, area, fish.then_some(20));
        }
    }
}

/// The setup's steps for screen readers, first in reading order as the column is on the left;
/// placed where [`column`] draws them.
pub(super) fn mark_nodes(builder: &mut Builder, page: &Page) {
    let Frame::Setup { marks, .. } = &page.frame else { return };
    let x = 1 + 24;
    let room = column_width(&page.frame) - 1 - 48;
    let mut y = MAIN_TOP + 24 + 64 + 12 + 30 + 32;
    for (index, mark) in marks.iter().enumerate() {
        let mut node = Node::new(
            Role::Text,
            mark.label.clone(),
            Rect::new(x, y, room, 16),
            format!("mark-{index}"),
        );
        node.status = Some(status(mark.state).to_owned());
        builder.node(node);
        y += 16 + 12;
    }
}

/// How a step's state is read out.
pub(super) fn status(state: State) -> &'static str {
    match state {
        State::Done => "完成",
        State::Now => "进行中",
        State::Todo => "未开始",
    }
}

fn marker(builder: &mut Builder, x: i32, y: i32, state: State) {
    let rect = Rect::new(x, y, 12, 12);
    match state {
        State::Todo => builder.scene.frame(rect, FAINT),
        State::Now => builder.scene.fill(rect, ACCENT),
        State::Done => {
            builder.scene.fill(rect, ACCENT);
            builder.scene.art(&art::TICK, x, y, 1, WHITE, true);
        }
    }
}

/// The shark, when the column has one (`fish` is its distance from the column's left), above
/// three dithered bands: a quarter, a half, then the full deep band.
fn sea(builder: &mut Builder, area: Rect, fish: Option<i32>) {
    let width = area.width - 1;
    let top = area.bottom() - 32;
    if let Some(left) = fish {
        builder.scene.art(&art::FISH, area.x + left, top - 12 - 22, 2, INK, false);
    }
    dither(builder, area.x, top, width, 8, SEA, false);
    dither(builder, area.x, top + 8, width, 8, SEA, true);
    builder.scene.fill(Rect::new(area.x, top + 16, width, 16), SEA);
    dither(builder, area.x, top + 16, width, 16, SEA_DEEP, false);
}

/// 8 px tiles with their top-right quarter filled, and their bottom-left one too for `checker`.
fn dither(
    builder: &mut Builder,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    colour: u32,
    checker: bool,
) {
    let right = x + width;
    let square = |builder: &mut Builder, left: i32, top: i32| {
        let size = (right - left).min(4);
        if size > 0 {
            builder.scene.fill(Rect::new(left, top, size, 4), colour);
        }
    };
    for row in (0..height).step_by(8) {
        for tile in (0..width).step_by(8) {
            square(builder, x + tile + 4, y + row);
            if checker {
                square(builder, x + tile, y + row + 4);
            }
        }
    }
}
