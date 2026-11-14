//! A program's menu in front still has the program in front, through its window.
use aura_desktop_control::arrange::{Shown, Step, step};
use aura_desktop_control::front::{Window, in_front};
use aura_desktop_domain::WindowAction;

const fn window(process_id: u32, listed: bool, foreground: bool) -> Window {
    Window { process_id, listed, foreground }
}

#[test]
fn the_listed_window_in_front_is_in_front() {
    let windows = [window(7, false, false), window(3, true, false), window(7, true, true)];
    assert_eq!(in_front(&windows), Some(2));
}

#[test]
fn a_menu_in_front_counts_for_the_frontmost_window_of_its_process() {
    let windows = [
        window(7, false, true),
        window(3, true, false),
        window(7, true, false),
        window(7, true, false),
    ];
    assert_eq!(in_front(&windows), Some(2));
    // `front` then leaves that window, and its menu, as they are.
    let shown = Shown { minimized: false, maximized: false };
    assert_eq!(step(WindowAction::Front, shown, in_front(&windows) == Some(2)), Step::Nothing);
}

#[test]
fn nothing_is_in_front_without_a_listed_window_of_the_process_in_front() {
    assert_eq!(in_front(&[window(7, false, true), window(3, true, false)]), None);
    assert_eq!(in_front(&[window(0, false, true), window(0, true, false)]), None);
    assert_eq!(in_front(&[window(3, true, false)]), None);
    assert_eq!(in_front(&[]), None);
}
