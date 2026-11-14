//! What a window action needs of the window, what is sent for it and when the window shows it:
//! the rules alone, with no desktop.
use aura_desktop_control::arrange::*;
use aura_desktop_domain::WindowAction::{self, Close, Front, Maximize, Minimize, Restore};

const NORMAL: Shown = Shown { minimized: false, maximized: false };
const MINIMIZED: Shown = Shown { minimized: true, maximized: false };
const MAXIMIZED: Shown = Shown { minimized: false, maximized: true };

#[test]
fn a_window_already_as_asked_is_sent_nothing() {
    assert_eq!(step(Minimize, MINIMIZED, false), Step::Nothing);
    assert_eq!(step(Maximize, MAXIMIZED, true), Step::Nothing);
    assert_eq!(step(Restore, NORMAL, false), Step::Nothing);
    assert_eq!(step(Front, NORMAL, true), Step::Nothing);
}

#[test]
fn a_minimize_leaves_the_window_in_front_where_it_is_unless_it_is_that_window() {
    assert_eq!(step(Minimize, NORMAL, true), Step::Show(Show::Minimize));
    assert_eq!(step(Minimize, MAXIMIZED, false), Step::Show(Show::MinimizeBehind));
    assert_eq!(step(Maximize, MINIMIZED, false), Step::Show(Show::Maximize));
    assert_eq!(step(Restore, MAXIMIZED, true), Step::Show(Show::Normal));
    assert_eq!(step(Restore, MINIMIZED, false), Step::Show(Show::Normal));
    assert_eq!(step(Front, NORMAL, false), Step::Front);
    assert_eq!(step(Front, MINIMIZED, true), Step::Front, "minimized, even with the focus");
    assert_eq!(step(Close, NORMAL, true), Step::Close);
    assert_eq!(step(Close, MINIMIZED, false), Step::Close);
}

#[test]
fn maximizing_brings_the_window_to_the_front_even_when_it_already_fills_the_screen() {
    assert_eq!(step(Maximize, MAXIMIZED, false), Step::Front);
    assert_eq!(step(Maximize, MAXIMIZED, true), Step::Nothing);
    assert!(brings_to_front(Maximize) && brings_to_front(Front));
    for action in [Minimize, Restore, Close] {
        assert!(!brings_to_front(action), "{action:?}");
    }
}

#[test]
fn a_size_change_is_done_once_the_window_shows_it() {
    let cases: [(WindowAction, Shown, bool); 8] = [
        (Minimize, MINIMIZED, true),
        (Minimize, NORMAL, false),
        (Maximize, MAXIMIZED, true),
        (Maximize, Shown { minimized: true, maximized: true }, false),
        (Maximize, NORMAL, false),
        (Restore, NORMAL, true),
        (Restore, MINIMIZED, false),
        (Restore, MAXIMIZED, false),
    ];
    for (action, now, done) in cases {
        assert_eq!(shows(action, now), done, "{action:?} {now:?}");
    }
}

#[test]
fn a_window_without_the_button_is_refused_only_that_size() {
    let both = Buttons { minimize: true, maximize: true };
    let none = Buttons { minimize: false, maximize: false };
    for action in [Front, Minimize, Maximize, Restore, Close] {
        assert_eq!(refusal(action, both), None, "{action:?}");
    }
    for action in [Front, Restore, Close] {
        assert_eq!(refusal(action, none), None, "{action:?}");
    }
    assert!(refusal(Minimize, none).is_some_and(|why| why.contains("最小化")));
    let fixed = Buttons { minimize: true, maximize: false };
    assert!(refusal(Maximize, fixed).is_some_and(|why| why.contains("最大化")));
    assert_eq!(refusal(Minimize, fixed), None);
}
