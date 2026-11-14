//! The page's own content: the heading, then its blocks 20 px apart, inside 28 px of padding at
//! the top and sides and 24 px at the bottom.
use super::{Builder, Node, Role, controls, frame};
use crate::{
    art,
    page::{Block, Face, Field, Line, Meter, Page, Pair, State, Step, Tone},
    scene::{Item, Rect},
    theme::{
        ACCENT, CONTROL_LINE, DANGER, INK, LINE, LINE_SOFT, MUTED, PIXEL_SHADOW, QUIET, WHITE,
    },
};

const GAP: i32 = 20;

/// Lays the body out in `content` (its height is ignored); returns the height it takes.
pub(super) fn lay_out(builder: &mut Builder, page: &Page, content: Rect) -> i32 {
    let x = content.x + 28;
    let width = content.width - 56;
    let mut y = content.y + 28;
    let heading = builder.wrapped(&page.heading, (Face::Pixel, 24, 30), (INK, WHITE), x, y, width);
    builder.node(Node::new(
        Role::Heading,
        page.heading.clone(),
        Rect::new(x, y, width, heading),
        "heading",
    ));
    y += heading;
    for (index, block) in page.blocks.iter().enumerate() {
        y += GAP;
        y += match block {
            Block::Field(item) => field(builder, item, x, y, width, index),
            Block::Check(item) => controls::check(builder, item, x, y, width),
            Block::Meter(item) => meter(builder, item, &page.heading, x, y, width),
            Block::Steps(items) => steps(builder, items, x, y, width, index),
            Block::Pairs(items) => pairs(builder, items, x, y, width, index),
            Block::Text(item) => line(builder, item, x, y, width, index),
        };
    }
    y + 24 - content.y
}

fn field(builder: &mut Builder, field: &Field, x: i32, y: i32, width: i32, index: usize) -> i32 {
    let mut top = y;
    if field.show_label {
        builder.words(
            &field.label,
            (Face::Pixel, 12, 16),
            (MUTED, WHITE),
            x,
            top,
            Some(width),
            false,
        );
        top += 16 + 8;
    }
    let button = field.button.as_ref().map(|button| controls::button_width(builder, button));
    let input = width - button.map_or(0, |button| button + 8);
    let rect = Rect::new(x, top, input, 32);
    if field.changed {
        builder.scene.fill(rect.offset(2, 2), PIXEL_SHADOW);
    }
    builder.scene.fill(rect, WHITE);
    builder.scene.frame(rect, if field.changed { INK } else { CONTROL_LINE });
    builder.words(
        &field.value,
        (Face::Reading, 13, 30),
        (INK, WHITE),
        x + 11,
        top + 1,
        Some(input - 22),
        false,
    );
    let mut node = Node::new(Role::Edit, field.label.clone(), rect, format!("field-{index}"));
    node.value = Some(field.value.clone());
    builder.node(node);
    if let Some(item) = &field.button {
        let name = format!("{}{}", item.label, field.label);
        controls::button(builder, item, x + input + 8, top, name);
    }
    top += 32;
    if !field.detail.is_empty() {
        top += 8;
        let mut left = x;
        for (text, face) in &field.detail {
            let room = x + width - left;
            if room <= 0 {
                break;
            }
            // Each part takes only its own width, so a part's box never holds the next one's ink.
            let own = builder.width(text, *face, 12).min(room);
            left +=
                builder.words(text, (*face, 12, 16), (MUTED, WHITE), left, top, Some(own), false)
                    + 6;
        }
        let words: Vec<&str> = field.detail.iter().map(|(text, _)| text.as_str()).collect();
        builder.node(Node::new(
            Role::Text,
            words.join(" "),
            Rect::new(x, top, width, 16),
            format!("detail-{index}"),
        ));
        top += 16;
    }
    top - y
}

/// The track: a dark edge, 2 px of white, then 6 px segments every 8 px up to the done part. A
/// bar without a position lights a run of segments that steps along with the animation.
fn meter(builder: &mut Builder, meter: &Meter, heading: &str, x: i32, y: i32, width: i32) -> i32 {
    let words = !(meter.left.is_empty() && meter.right.is_empty());
    let (words_top, track_top) = match (words, meter.words_above) {
        (false, _) => (y, y),
        (true, true) => (y, y + 16 + GAP),
        (true, false) => (y + 16 + GAP, y),
    };
    let track = Rect::new(x, track_top, width, 16);
    builder.scene.fill(track, WHITE);
    builder.scene.frame(track, INK);
    let inner = Rect::new(x + 3, track_top + 3, width - 6, 10);
    let (from, to) = match meter.fraction {
        Some(fraction) => (0, (fraction.clamp(0.0, 1.0) * f64::from(inner.width)).round() as i32),
        None => {
            let run = 48;
            let start = (builder.look.phase as i32 * 8) % (inner.width + run) - run;
            (start, start + run)
        }
    };
    let mut segment = 0;
    while segment < inner.width {
        let left = segment.max(from).max(0);
        let right = (segment + 6).min(to).min(inner.width);
        if right > left {
            builder
                .scene
                .fill(Rect::new(inner.x + left, inner.y, right - left, inner.height), ACCENT);
        }
        segment += 8;
    }
    if words {
        builder.words(
            &meter.left,
            (Face::Pixel, 12, 16),
            (MUTED, WHITE),
            x,
            words_top,
            Some(width),
            false,
        );
        builder.words(
            &meter.right,
            (Face::Pixel, 12, 16),
            (MUTED, WHITE),
            x,
            words_top,
            Some(width),
            true,
        );
    }
    let name = if meter.left.is_empty() { heading.to_owned() } else { meter.left.clone() };
    let mut node = Node::new(Role::ProgressBar, name, track, "progress");
    node.range = meter.fraction.map(|fraction| (fraction.clamp(0.0, 1.0) * 100.0).round());
    node.help = (!meter.right.is_empty()).then(|| meter.right.clone());
    builder.node(node);
    if words { 16 + GAP + 16 } else { 16 }
}

fn steps(builder: &mut Builder, steps: &[Step], x: i32, y: i32, width: i32, index: usize) -> i32 {
    let mut top = y;
    for (number, step) in steps.iter().enumerate() {
        if number > 0 {
            top += 8;
        }
        match step.state {
            State::Done => builder.scene.art(&art::CHECK, x, top, 1, INK, true),
            State::Now => builder.scene.items.push(Item::Spinner {
                x,
                y: top,
                phase: builder.look.phase,
                colour: INK,
            }),
            State::Todo => builder.scene.art(&art::TODO, x, top, 1, QUIET, true),
        }
        let ink = if step.state == State::Todo { QUIET } else { INK };
        let value =
            step.value.as_deref().map(|value| (value, builder.width(value, Face::Pixel, 12)));
        let room = width - 24 - value.map_or(0, |(_, width)| width + 8);
        builder.words(
            &step.label,
            (Face::Pixel, 12, 16),
            (ink, WHITE),
            x + 24,
            top,
            Some(room),
            false,
        );
        if let Some((value, value_width)) = value {
            builder.words(
                value,
                (Face::Pixel, 12, 16),
                (MUTED, WHITE),
                x + width - value_width,
                top,
                Some(value_width),
                false,
            );
        }
        let name = match &step.value {
            Some(value) => format!("{} {value}", step.label),
            None => step.label.clone(),
        };
        let mut node = Node::new(
            Role::Text,
            name,
            Rect::new(x, top, width, 16),
            format!("step-{index}-{number}"),
        );
        node.status = Some(frame::status(step.state).to_owned());
        builder.node(node);
        top += 16;
    }
    top - y
}

/// A bordered table, rows 36 px high with a soft line between them: the key, the path in the
/// reading face, and the size at the right.
fn pairs(builder: &mut Builder, pairs: &[Pair], x: i32, y: i32, width: i32, index: usize) -> i32 {
    if pairs.is_empty() {
        return 0;
    }
    let key = pairs.iter().map(|pair| builder.width(&pair.key, Face::Pixel, 12)).max().unwrap_or(0);
    let key = (key + 15) / 16 * 16;
    let rows = pairs.len() as i32;
    let height = 2 + rows * 36 + (rows - 1);
    builder.scene.frame(Rect::new(x, y, width, height), LINE);
    let mut top = y + 1;
    for (number, pair) in pairs.iter().enumerate() {
        if number > 0 {
            builder.scene.fill(Rect::new(x + 1, top, width - 2, 1), LINE_SOFT);
            top += 1;
        }
        let words = top + 10;
        builder.words(
            &pair.key,
            (Face::Pixel, 12, 16),
            (MUTED, WHITE),
            x + 13,
            words,
            Some(key),
            false,
        );
        let extra =
            pair.extra.as_deref().map(|extra| (extra, builder.width(extra, Face::Pixel, 12)));
        let value = x + 13 + key + 12;
        let end = x + width - 13 - extra.map_or(0, |(_, width)| width + 12);
        builder.words(
            &pair.value,
            (Face::Reading, 13, 16),
            (INK, WHITE),
            value,
            words,
            Some(end - value),
            false,
        );
        if let Some((extra, extra_width)) = extra {
            builder.words(
                extra,
                (Face::Pixel, 12, 16),
                (INK, WHITE),
                x + width - 13 - extra_width,
                words,
                Some(extra_width),
                false,
            );
        }
        let name = [Some(pair.key.as_str()), Some(pair.value.as_str()), pair.extra.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        builder.node(Node::new(
            Role::Text,
            name,
            Rect::new(x, top, width, 36),
            format!("pair-{index}-{number}"),
        ));
        top += 36;
    }
    height
}

fn line(builder: &mut Builder, line: &Line, x: i32, y: i32, width: i32, index: usize) -> i32 {
    let ink = match line.tone {
        Tone::Ink => INK,
        Tone::Muted => MUTED,
        Tone::Danger => DANGER,
    };
    let height = builder.wrapped(&line.text, (line.face, 12, 16), (ink, WHITE), x, y, width);
    builder.node(Node::new(
        Role::Text,
        line.text.clone(),
        Rect::new(x, y, width, height),
        format!("text-{index}"),
    ));
    height
}
