//! The pixel look's checks, as the desktop's own audit has them, on a drawn window: type only at
//! the faces' sizes, square corners, a shadow without blur, icons at the icon sizes, and words
//! whose edges blend only between their own colour and what they sit on (no ClearType colours).
use crate::{
    image::Image,
    layout::{Laid, Role},
    page::Face,
    scene::{Item, Rect, Text},
    text::Fonts,
    theme::{ICON_SIZES, INK, PIXEL_SIZES, READING_SIZES, SHADOW_ALPHA, SHADOW_OFFSET},
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Finding {
    pub what: &'static str,
    pub place: String,
    pub value: String,
}

fn finding(what: &'static str, place: impl Into<String>, value: impl Into<String>) -> Finding {
    Finding { what, place: place.into(), value: value.into() }
}

/// Every check on `image`, the scene of `laid` drawn at scale `k`.
pub fn audit(laid: &Laid, image: &Image, k: i32) -> Vec<Finding> {
    let mut found = Vec::new();
    for item in &laid.scene.items {
        match item {
            Item::Text(text) => {
                let sizes: &[i32] = match text.face {
                    Face::Pixel => &PIXEL_SIZES,
                    Face::Reading => &READING_SIZES,
                };
                if !sizes.contains(&text.size) {
                    found.push(finding(
                        "type",
                        &text.text,
                        format!("{:?} {}px", text.face, text.size),
                    ));
                }
                fringes(text, image, k, &mut found);
            }
            Item::Art { art, cell, icon: true, .. } => {
                let (width, height) = (art.grid.0 * cell, art.grid.1 * cell);
                if width != height || !ICON_SIZES.contains(&width) {
                    found.push(finding("icon", art.name, format!("{width}x{height}")));
                }
            }
            _ => {}
        }
    }
    corners(laid, image, k, &mut found);
    shadow(laid, image, k, &mut found);
    rows(laid, &mut found);
    found
}

/// Words in the pixel face whose characters the face lacks.
pub fn missing_glyphs(laid: &Laid, fonts: &Fonts) -> Vec<Finding> {
    laid.scene
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Text(text) if text.face == Face::Pixel => {
                let missing = fonts.missing_from_pixel_face(&text.text);
                (!missing.is_empty())
                    .then(|| finding("glyph", &text.text, missing.into_iter().collect::<String>()))
            }
            _ => None,
        })
        .collect()
}

fn channels(pixel: u32) -> [i32; 3] {
    [((pixel >> 16) & 0xff) as i32, ((pixel >> 8) & 0xff) as i32, (pixel & 0xff) as i32]
}

/// Each opaque pixel in the words' box lies on the line from the background to the words'
/// colour, at the same point for every channel; in the pixel face it is one end or the other.
fn fringes(text: &Text, image: &Image, k: i32, found: &mut Vec<Finding>) {
    let ink = channels(text.colour);
    let paper = channels(text.background);
    let spans: Vec<(usize, i32)> = (0..3)
        .map(|channel| (channel, paper[channel] - ink[channel]))
        .filter(|(_, span)| span.abs() >= 16)
        .collect();
    if spans.is_empty() {
        return;
    }
    let area = text.bounds().scaled(k);
    let (mut coloured, mut blended) = (0, 0);
    for y in area.y.max(0)..area.bottom().min(image.height as i32) {
        for x in area.x.max(0)..area.right().min(image.width as i32) {
            let pixel = image.pixel(x as u32, y as u32);
            if pixel >> 24 != 0xff {
                continue;
            }
            let value = channels(pixel);
            let at: Vec<f64> = spans
                .iter()
                .map(|(channel, span)| {
                    f64::from(paper[*channel] - value[*channel]) / f64::from(*span)
                })
                .collect();
            let (low, high) =
                at.iter().fold((f64::MAX, f64::MIN), |(low, high), t| (low.min(*t), high.max(*t)));
            if high - low > 0.08 || low < -0.05 || high > 1.05 {
                coloured += 1;
            } else if text.face == Face::Pixel && low > 0.02 && high < 0.98 {
                blended += 1;
            }
        }
    }
    if coloured > 0 {
        found.push(finding("fringe", &text.text, format!("{coloured} pixels off the colour line")));
    }
    if blended > 0 {
        found.push(finding(
            "blur",
            &text.text,
            format!("{blended} pixel-face pixels between colours"),
        ));
    }
}

/// A corner pixel has the colour of the edge pixels beside it; a rounded corner would show what
/// is behind it.
fn corners(laid: &Laid, image: &Image, k: i32, found: &mut Vec<Finding>) {
    let mut boxes: Vec<(String, Rect)> =
        vec![("window".to_owned(), Rect::new(0, 0, laid.scene.width, laid.scene.height))];
    for node in &laid.nodes {
        match node.role {
            Role::Button | Role::Edit | Role::ProgressBar if node.automation_id != "close" => {
                boxes.push((node.name.clone(), node.rect));
            }
            Role::CheckBox => {
                boxes.push((node.name.clone(), Rect::new(node.rect.x, node.rect.y + 1, 14, 14)))
            }
            _ => {}
        }
    }
    let at = |x: i32, y: i32| image.pixel((x * k) as u32, (y * k) as u32);
    for (name, area) in boxes {
        let (left, top, right, bottom) = (area.x, area.y, area.right() - 1, area.bottom() - 1);
        let corners = [
            (left, top, left + 1, top + 1),
            (right, top, right - 1, top + 1),
            (left, bottom, left + 1, bottom - 1),
            (right, bottom, right - 1, bottom - 1),
        ];
        for (x, y, beside_x, beside_y) in corners {
            let corner = at(x, y);
            if corner != at(beside_x, y) || corner != at(x, beside_y) {
                found.push(finding("radius", &name, format!("corner {x},{y}")));
            }
        }
    }
    let window = at(0, 0);
    if window != (0xff00_0000 | INK) {
        found.push(finding("radius", "window", format!("{window:08x}")));
    }
}

/// The shadow is one flat colour at its strength, and nothing lies beyond it.
fn shadow(laid: &Laid, image: &Image, k: i32, found: &mut Vec<Finding>) {
    let (width, height, offset) = (laid.scene.width * k, laid.scene.height * k, SHADOW_OFFSET * k);
    let mut values = std::collections::BTreeSet::new();
    for y in offset..height + offset {
        for x in width..width + offset {
            values.insert(image.pixel(x as u32, y as u32));
        }
    }
    for y in height..height + offset {
        for x in offset..width {
            values.insert(image.pixel(x as u32, y as u32));
        }
    }
    if values.len() != 1 {
        found.push(finding("shadow", "window", format!("{} values", values.len())));
    }
    let expected = (SHADOW_ALPHA * 255.0).round() as u32;
    if let Some(value) = values.first()
        && (value >> 24).abs_diff(expected) > 1
    {
        found.push(finding("shadow", "window", format!("alpha {}", value >> 24)));
    }
    let clear = (0..offset)
        .flat_map(|y| (width..width + offset).map(move |x| (x, y)))
        .chain((height..height + offset).flat_map(|y| (0..offset).map(move |x| (x, y))))
        .filter(|(x, y)| image.pixel(*x as u32, *y as u32) != 0)
        .count();
    if clear > 0 {
        found.push(finding("shadow", "window", format!("{clear} pixels beyond the shadow")));
    }
}

/// A button beside a field has the field's height.
fn rows(laid: &Laid, found: &mut Vec<Finding>) {
    for pair in laid.nodes.windows(2) {
        let (field, next) = (&pair[0], &pair[1]);
        if field.role == Role::Edit
            && next.role == Role::Button
            && next.rect.y == field.rect.y
            && next.rect.height != field.rect.height
        {
            found.push(finding(
                "row",
                &next.name,
                format!("{} beside {}", next.rect.height, field.rect.height),
            ));
        }
    }
}
