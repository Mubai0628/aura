//! Icons as the Shell draws them, made fit for a PNG.
use aura_desktop_control::alpha::straighten;

#[test]
fn premultiplied_colours_are_made_straight_and_unseen_ones_are_cleared() {
    let mut pixels = vec![
        0, 0, 128, 128, // half-transparent red, premultiplied
        10, 20, 30, 255, // opaque
        7, 7, 7, 0, // transparent, with leftover colour
        64, 0, 0, 64, // a quarter blue
    ];
    straighten(&mut pixels);
    assert_eq!(pixels, [0, 0, 255, 128, 10, 20, 30, 255, 0, 0, 0, 0, 255, 0, 0, 64]);
}

#[test]
fn an_icon_drawn_without_alpha_is_opaque_and_an_empty_image_stays_empty() {
    let mut old = vec![10, 20, 30, 0, 0, 0, 0, 0];
    straighten(&mut old);
    assert_eq!(old, [10, 20, 30, 255, 0, 0, 0, 255]);
    let mut empty = vec![0; 8];
    straighten(&mut empty);
    assert_eq!(empty, [0; 8]);
}
