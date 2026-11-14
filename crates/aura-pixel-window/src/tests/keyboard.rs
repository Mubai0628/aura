//! The keyboard alone works a window: Tab moves the focus, Space presses or ticks, Enter takes
//! the focused button or the primary one, Esc closes a page that can be closed.
use std::{thread, time::Duration};

use super::{install_page, launcher_progress_page, one_window_at_a_time};
use crate::{
    page::{Action, Page},
    testing::{Key, press, wait_for_window, windows},
    window::PixelWindow,
};

/// Shows `page` on a thread of its own and returns what it ended with and whether
/// "shortcut" was checked.
fn shown(page: Page) -> (isize, thread::JoinHandle<(Action, bool)>) {
    let thread = thread::spawn(move || {
        let mut page = page;
        let mut window = PixelWindow::open(&page).expect("the window opens");
        let action = window.show(&mut page).expect("the page runs");
        (action, page.checked("shortcut"))
    });
    let window = wait_for_window(Duration::from_secs(10)).expect("the window appears");
    (window, thread)
}

#[test]
fn tab_space_and_enter_tick_the_box_and_install() {
    let _one = one_window_at_a_time();
    let (window, thread) = shown(install_page());
    // The focus starts on 安装; the stops are 更改, the box, the link, 取消, 安装.
    for key in [Key::Tab, Key::Tab, Key::Space, Key::Tab, Key::Tab, Key::Tab, Key::Enter] {
        press(window, key);
    }
    let (action, checked) = thread.join().expect("the window's thread ends");
    assert_eq!(action, Action::Button("install"));
    assert!(checked, "Space on the box ticked it");
    assert!(windows().is_empty(), "the window is gone");
}

#[test]
fn enter_on_a_check_box_takes_the_primary_button_and_down_moves_on() {
    let _one = one_window_at_a_time();
    let (window, thread) = shown(install_page());
    for key in [Key::Down, Key::Down, Key::Enter] {
        press(window, key);
    }
    let (action, checked) = thread.join().expect("the window's thread ends");
    assert_eq!(action, Action::Button("install"));
    assert!(!checked, "Enter does not tick the box it is on");
}

#[test]
fn escape_closes_a_page_that_can_be_closed() {
    let _one = one_window_at_a_time();
    let (window, thread) = shown(install_page());
    press(window, Key::Escape);
    assert_eq!(thread.join().expect("ends").0, Action::Close);
}

#[test]
fn escape_does_nothing_while_work_runs_and_the_page_ends_with_the_work() {
    let _one = one_window_at_a_time();
    let thread = thread::spawn(|| {
        let page = launcher_progress_page();
        let mut window = PixelWindow::open(&page).expect("the window opens");
        window.run_work(
            &page,
            |_| {},
            || {
                thread::sleep(Duration::from_millis(600));
                42
            },
        )
    });
    let window = wait_for_window(Duration::from_secs(10)).expect("the window appears");
    press(window, Key::Escape);
    assert_eq!(thread.join().expect("ends"), 42);
}
