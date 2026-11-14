//! Each sample window drawn offscreen at 100, 125, 150 and 200 %, through the five checks; and
//! the checks themselves catching what they look for.
use std::path::PathBuf;

use super::{every_block_page, install_page, launcher_page, launcher_progress_page};
use crate::{
    art,
    audit::{Finding, audit, missing_glyphs},
    draw, fonts,
    page::{Face, Page},
    scale::whole_scale,
    scene::{Item, Text},
};

const DPIS: [u32; 4] = [96, 120, 144, 192];

/// Where the drawn windows are written for a person to look at, when a run asks for them.
fn shots() -> Option<PathBuf> {
    std::env::var_os("AURA_PIXEL_SHOTS").map(PathBuf::from)
}

/// Draws `page` at each DPI and returns what the checks found, by DPI.
pub(crate) fn findings(name: &str, page: &Page) -> Vec<(u32, Vec<Finding>)> {
    DPIS.iter()
        .map(|dpi| {
            let k = whole_scale(*dpi);
            let (laid, image) = draw(page, k).expect("the window draws");
            if let Some(folder) = shots() {
                let _ = std::fs::create_dir_all(&folder);
                let _ = image.save_png(&folder.join(format!("{name}-{dpi}dpi.png")));
            }
            let mut found = audit(&laid, &image, k);
            found.extend(missing_glyphs(&laid, &fonts().expect("fonts")));
            (*dpi, found)
        })
        .collect()
}

#[test]
fn every_sample_window_passes_the_checks_at_each_scale() {
    for (name, page) in [
        ("install", install_page()),
        ("every-block", every_block_page()),
        ("launcher", launcher_page()),
        ("launcher-progress", launcher_progress_page()),
    ] {
        for (dpi, found) in findings(name, &page) {
            assert!(found.is_empty(), "{name} at {dpi} DPI: {found:#?}");
        }
    }
}

#[test]
fn at_200_percent_the_frame_and_pixel_words_are_the_100_percent_ones_doubled() {
    let page = install_page();
    let (laid, small) = draw(&page, 1).expect("100 %");
    let (_, large) = draw(&page, 2).expect("200 %");
    assert_eq!((large.width, large.height), (small.width * 2, small.height * 2));
    // The column, the title bar and the footer hold only the pixel face and pixel art.
    let footer_top = (laid.scene.height - 1 - 69) as u32;
    let same = |x: u32, y: u32| {
        let pixel = small.pixel(x, y);
        (0..2).all(|dy| (0..2).all(|dx| large.pixel(2 * x + dx, 2 * y + dy) == pixel))
    };
    for y in 0..small.height {
        for x in 0..small.width {
            let column = x < 233;
            let bar = y < 34;
            let footer = y >= footer_top && y < laid.scene.height as u32;
            if column || bar || footer {
                assert!(same(x, y), "pixel {x},{y} differs between 100 % and 200 %");
            }
        }
    }
}

#[test]
fn the_checks_catch_wrong_sizes_rounded_corners_blur_and_coloured_edges() {
    let (mut laid, image) = draw(&install_page(), 1).expect("draws");
    assert!(audit(&laid, &image, 1).is_empty());
    let kinds =
        |found: Vec<Finding>| found.into_iter().map(|finding| finding.what).collect::<Vec<_>>();

    let mut odd = laid.scene.clone();
    odd.items.push(Item::Text(Text {
        text: "路径".to_owned(),
        face: Face::Reading,
        size: 14,
        colour: 0x1c1c1b,
        background: 0xffffff,
        x: 300,
        top: 300,
        width: 40,
        line: 16,
        lines: 1,
        baseline: 12,
        wrap: false,
        right_align: false,
    }));
    odd.items.push(Item::Art { art: &art::CLOSE, x: 0, y: 0, cell: 3, colour: 0, icon: true });
    let original = std::mem::replace(&mut laid.scene, odd);
    let found = kinds(audit(&laid, &image, 1));
    assert!(found.contains(&"type"), "{found:?}");
    assert!(found.contains(&"icon"), "{found:?}");
    laid.scene = original;

    let text = laid
        .scene
        .items
        .iter()
        .find_map(|item| match item {
            Item::Text(text) if text.text == "安装" => Some(text.bounds()),
            _ => None,
        })
        .expect("the install button's words");
    let mut coloured = image.clone();
    let at = (text.y as u32 + 4) * coloured.width + text.x as u32 + 2;
    coloured.pixels[at as usize] = 0xffc87850;
    assert!(kinds(audit(&laid, &coloured, 1)).contains(&"fringe"));

    let mut blurred = image.clone();
    let shadow = (laid.scene.height as u32 + 2) * blurred.width + 100;
    blurred.pixels[shadow as usize] = 0x1a04_0404;
    assert!(kinds(audit(&laid, &blurred, 1)).contains(&"shadow"));

    let mut rounded = image.clone();
    rounded.pixels[0] = 0;
    assert!(kinds(audit(&laid, &rounded, 1)).contains(&"radius"));
}
