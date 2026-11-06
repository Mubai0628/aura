//! What a window action needs of the window, what is sent for it, and when the window shows it.
use aura_desktop_domain::WindowAction;

/// Whether a window is minimized or maximized now.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Shown {
    pub minimized: bool,
    pub maximized: bool,
}

/// The size buttons a window's title bar has.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Buttons {
    pub minimize: bool,
    pub maximize: bool,
}

/// How a size change is asked of the window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Show {
    /// As its minimize button does: the next window takes its place in front.
    Minimize,
    /// From behind other windows: the window in front stays in front.
    MinimizeBehind,
    Maximize,
    /// Its normal size, without coming to the front.
    Normal,
}

/// What the host does for an action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Step {
    /// The window already is as asked: nothing is sent.
    Nothing,
    Front,
    Show(Show),
    Close,
}

/// Why the window cannot do it, when it cannot.
#[must_use]
pub fn refusal(action: WindowAction, buttons: Buttons) -> Option<&'static str> {
    match action {
        WindowAction::Minimize if !buttons.minimize => Some("这个窗口没有最小化按钮，不能最小化"),
        WindowAction::Maximize if !buttons.maximize => Some("这个窗口没有最大化按钮，不能最大化"),
        _ => None,
    }
}

/// Whether the action brings the window to the front as well: front does, and maximize does as
/// a maximize button clicked by the user does.
#[must_use]
pub fn brings_to_front(action: WindowAction) -> bool {
    matches!(action, WindowAction::Front | WindowAction::Maximize)
}

#[must_use]
pub fn step(action: WindowAction, now: Shown, foreground: bool) -> Step {
    match action {
        WindowAction::Front if foreground && !now.minimized => Step::Nothing,
        WindowAction::Front => Step::Front,
        WindowAction::Close => Step::Close,
        WindowAction::Maximize if shows(action, now) && !foreground => Step::Front,
        _ if shows(action, now) => Step::Nothing,
        WindowAction::Minimize if foreground => Step::Show(Show::Minimize),
        WindowAction::Minimize => Step::Show(Show::MinimizeBehind),
        WindowAction::Maximize => Step::Show(Show::Maximize),
        WindowAction::Restore => Step::Show(Show::Normal),
    }
}

/// Whether the window shows the size a minimize, maximize or restore asked for.
#[must_use]
pub fn shows(action: WindowAction, now: Shown) -> bool {
    match action {
        WindowAction::Minimize => now.minimized,
        WindowAction::Maximize => now.maximized && !now.minimized,
        WindowAction::Restore => !now.minimized && !now.maximized,
        WindowAction::Front | WindowAction::Close => false,
    }
}
