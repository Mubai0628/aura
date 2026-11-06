//! Waiting, after a click or a key, for a window the program opens in answer (a menu, a dropdown,
//! a dialog) before the next step or the screenshot: a key sent while a menu is still opening
//! reaches the window behind it, and a click lands where the menu is not yet.
use crate::plan::Planned;
use aura_desktop_domain::input::VK_SHIFT;
use aura_desktop_domain::{InputStep, MouseButton, Rect};
use std::collections::BTreeSet;

/// How long a new window may take to appear after a click or a key.
pub const APPEAR_MS: u64 = 150;
/// After a right click or the menu key: a context menu may fade in.
pub const APPEAR_AFTER_MENU_MS: u64 = 300;
/// The longest wait after one step.
pub const LIMIT_MS: u64 = 600;
pub const POLL_MS: u64 = 30;

const VK_APPS: u16 = 0x5D;
const VK_F10: u16 = 0x79;

/// How long a window may take to appear after the step; none for a step that opens nothing of
/// its own (moving, dragging, scrolling, typing, waiting).
#[must_use]
pub fn appear_ms(step: &Planned) -> Option<u64> {
    match step {
        Planned::Step(InputStep::Click { button: MouseButton::Right, .. }) => {
            Some(APPEAR_AFTER_MENU_MS)
        }
        Planned::Step(InputStep::Click { .. }) => Some(APPEAR_MS),
        Planned::Chord(chord, _) => {
            let shift = chord.modifiers.iter().any(|key| key.code == VK_SHIFT);
            let menu = chord.key.code == VK_APPS || (shift && chord.key.code == VK_F10);
            Some(if menu { APPEAR_AFTER_MENU_MS } else { APPEAR_MS })
        }
        Planned::Step(_) => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Next {
    Wait,
    Go,
}

/// Whether to look again, given the program's top-level windows before the step, at the last
/// look and now, and the time since the step. A window that was not there before is waited for
/// until it is the same at two looks in a row; without one, until `appear_ms` has passed.
#[must_use]
pub fn next(
    before: &BTreeSet<isize>,
    last: &[(isize, Rect)],
    now: &[(isize, Rect)],
    elapsed_ms: u64,
    appear_ms: u64,
) -> Next {
    if elapsed_ms >= LIMIT_MS {
        return Next::Go;
    }
    let mut fresh = now.iter().filter(|(handle, _)| !before.contains(handle)).peekable();
    if fresh.peek().is_none() {
        return if elapsed_ms < appear_ms { Next::Wait } else { Next::Go };
    }
    if fresh.all(|window| last.contains(window)) { Next::Go } else { Next::Wait }
}

/// Looks at the program's top-level windows until [`next`] says to go. `wait` lets one
/// [`POLL_MS`] pass and gives the time since the step; `look` lists the windows.
pub fn settle(
    before: &BTreeSet<isize>,
    appear_ms: u64,
    mut wait: impl FnMut() -> u64,
    mut look: impl FnMut() -> Vec<(isize, Rect)>,
) {
    let mut last = Vec::new();
    loop {
        let elapsed_ms = wait();
        let now = look();
        if next(before, &last, &now, elapsed_ms, appear_ms) == Next::Go {
            return;
        }
        last = now;
    }
}
