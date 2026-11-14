//! Buttons, check boxes and links, in each of their states, and the footer that holds the
//! window's buttons.
use super::{Builder, Node, Role, Target};
use crate::{
    art,
    page::{Button, Check, Face, Kind, Link, Page},
    scene::Rect,
    theme::{
        ACCENT, ACCENT_HOVER, CONTROL_LINE, DANGER, DANGER_LINE, DANGER_SHADE, DANGER_SOFT, FAINT,
        INK, LINE, LINE_SOFT, LINE_STRONG, MUTED, PIXEL_SHADOW, SURFACE_MUTED, SURFACE_SELECTED,
        WHITE,
    },
};

const BUTTON_HEIGHT: i32 = 32;
/// The footer: its line, 16 above the buttons and 20 below.
const FOOTER_HEIGHT: i32 = 1 + 16 + BUTTON_HEIGHT + 20;
const GAP: i32 = 8;

pub(super) fn button_width(builder: &Builder, button: &Button) -> i32 {
    builder.width(&button.label, Face::Pixel, 12) + 2 * 14 + 2
}

/// Draws `button` with its top-left at `x`, `y`; `name` is what screen readers call it.
pub(super) fn button(builder: &mut Builder, button: &Button, x: i32, y: i32, name: String) -> i32 {
    let rect = Rect::new(x, y, button_width(builder, button), BUTTON_HEIGHT);
    let node = builder.node(Node::new(Role::Button, name, rect, format!("button-{}", button.id)));
    let index = builder.control(
        Target::Button(button.id),
        rect,
        button.enabled,
        button.kind == Kind::Primary,
        node,
    );
    let (hover, pressed, focused) = builder.state(index);
    let lit = hover || pressed;
    let (face, edge, ink, shade) = match (button.kind, button.enabled) {
        (Kind::Primary, false) => (SURFACE_SELECTED, SURFACE_SELECTED, FAINT, None),
        (_, false) => (SURFACE_MUTED, LINE, FAINT, None),
        (Kind::Normal, true) => (
            if pressed { SURFACE_MUTED } else { WHITE },
            if lit { INK } else { LINE_STRONG },
            INK,
            Some(PIXEL_SHADOW),
        ),
        (Kind::Primary, true) => {
            let face = if pressed {
                INK
            } else if hover {
                ACCENT_HOVER
            } else {
                ACCENT
            };
            (face, face, WHITE, Some(LINE_STRONG))
        }
        (Kind::Danger, true) => (
            if lit { DANGER_SOFT } else { WHITE },
            if lit { DANGER } else { DANGER_LINE },
            DANGER,
            Some(DANGER_SHADE),
        ),
    };
    // Held down, the button moves onto its shadow.
    let at = if pressed { rect.offset(2, 2) } else { rect };
    if let (Some(shade), false) = (shade, pressed) {
        builder.scene.fill(rect.offset(2, 2), shade);
    }
    builder.scene.fill(at, face);
    builder.scene.frame(at, edge);
    builder.words(
        &button.label,
        (Face::Pixel, 12, 16),
        (ink, face),
        at.x + 15,
        at.y + 8,
        Some(rect.width - 30),
        false,
    );
    if focused && builder.look.cues {
        builder.scene.ring(rect, 2, 2, INK);
    }
    rect.width
}

/// A check box and its words, 16 px high; the words are part of what the pointer reaches.
pub(super) fn check(builder: &mut Builder, check: &Check, x: i32, y: i32, width: i32) -> i32 {
    let words = builder.width(&check.label, Face::Pixel, 12).min(width - 22);
    let rect = Rect::new(x, y, 22 + words, 16);
    let mut node =
        Node::new(Role::CheckBox, check.label.clone(), rect, format!("check-{}", check.id));
    node.toggled = Some(check.checked);
    let node = builder.node(node);
    let index = builder.control(Target::Check(check.id), rect, check.enabled, false, node);
    let (hover, _, focused) = builder.state(index);
    let tick = Rect::new(x, y + 1, 14, 14);
    if check.checked {
        builder.scene.fill(tick, if check.enabled { ACCENT } else { FAINT });
        builder.scene.art(&art::TICK, tick.x + 1, tick.y + 1, 1, WHITE, true);
    } else {
        builder.scene.fill(tick, if check.enabled { WHITE } else { SURFACE_MUTED });
        let edge = if !check.enabled {
            LINE
        } else if hover {
            INK
        } else {
            CONTROL_LINE
        };
        builder.scene.frame(tick, edge);
    }
    let ink = if check.enabled { INK } else { FAINT };
    builder.words(
        &check.label,
        (Face::Pixel, 12, 16),
        (ink, WHITE),
        x + 22,
        y,
        Some(width - 22),
        false,
    );
    if focused && builder.look.cues {
        builder.scene.ring(tick, 2, 2, INK);
    }
    16
}

/// An underlined link on a 16 px line whose top is `y`.
fn link(builder: &mut Builder, link: &Link, x: i32, y: i32) -> i32 {
    let width = builder.width(&link.label, Face::Pixel, 12);
    let rect = Rect::new(x, y, width, 16);
    let node = builder.node(Node::new(
        Role::Hyperlink,
        link.label.clone(),
        rect,
        format!("link-{}", link.id),
    ));
    let index = builder.control(Target::Link(link.id), rect, true, false, node);
    let (hover, _, focused) = builder.state(index);
    let ink = if hover { INK } else { MUTED };
    builder.words(&link.label, (Face::Pixel, 12, 16), (ink, WHITE), x, y, None, false);
    let baseline = y + builder.measure.baseline(Face::Pixel, 12, 16);
    builder.scene.fill(Rect::new(x, baseline + 2, width, 1), ink);
    if focused && builder.look.cues {
        builder.scene.ring(rect, 2, 2, INK);
    }
    width
}

fn empty(page: &Page) -> bool {
    page.footer.link.is_none() && page.footer.start.is_empty() && page.footer.end.is_empty()
}

pub(super) fn footer_height(page: &Page) -> i32 {
    if empty(page) { 0 } else { FOOTER_HEIGHT }
}

/// The narrowest content column the footer fits in: its padding, its items and their gaps, with
/// a gap more on each side of the space between the two groups.
pub(super) fn footer_width(builder: &mut Builder, page: &Page) -> i32 {
    let footer = &page.footer;
    let link = footer.link.as_ref().map(|link| builder.width(&link.label, Face::Pixel, 12));
    let buttons: Vec<i32> = page.buttons().map(|button| button_width(builder, button)).collect();
    let items = buttons.len() + usize::from(link.is_some());
    if items == 0 {
        return 0;
    }
    let spread = !footer.end.is_empty() && (link.is_some() || !footer.start.is_empty());
    let gaps = (items as i32 - 1 + i32::from(spread)) * GAP;
    2 * 28 + link.unwrap_or(0) + buttons.iter().sum::<i32>() + gaps
}

pub(super) fn footer(builder: &mut Builder, page: &Page, area: Rect) {
    if empty(page) {
        return;
    }
    builder.scene.fill(Rect::new(area.x, area.y, area.width, 1), LINE_SOFT);
    let y = area.y + 1 + 16;
    let mut x = area.x + 28;
    if let Some(item) = &page.footer.link {
        x += link(builder, item, x, y + 8) + GAP;
    }
    for item in &page.footer.start {
        x += button(builder, item, x, y, item.label.clone()) + GAP;
    }
    let widths: Vec<i32> = page.footer.end.iter().map(|item| button_width(builder, item)).collect();
    let total = widths.iter().sum::<i32>() + GAP * (widths.len() as i32 - 1).max(0);
    let mut x = area.right() - 28 - total;
    for (item, width) in page.footer.end.iter().zip(widths) {
        button(builder, item, x, y, item.label.clone());
        x += width + GAP;
    }
}
