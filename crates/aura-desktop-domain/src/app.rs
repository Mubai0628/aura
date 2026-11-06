//! Programs and their windows, as the desktop host reports them.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Where a program in the catalog comes from.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppKind {
    /// A packaged (Store) app of the Start menu's All apps; its ID is an AUMID such as
    /// `Microsoft.WindowsNotepad_8wekyb3d8bbwe!App`.
    Packaged,
    /// Any other program of All apps; its ID is an AUMID the program declares or a path that
    /// starts with a known folder's GUID.
    Desktop,
    /// A program the user added by its path; its ID is that path.
    Manual,
}

/// One program the user may let Aura operate.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppEntry {
    pub id: String,
    /// Its name as the Start menu shows it, such as 记事本.
    pub name: String,
    pub kind: AppKind,
    /// The program file, when Windows knows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// A program the user added in the settings, by its path.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ManualApp {
    pub name: String,
    pub path: String,
}

/// A program's icon as the Start menu shows it.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AppIcon {
    pub app_id: String,
    /// A PNG, base64.
    pub png: String,
}

/// A rectangle in physical pixels of the virtual screen (every monitor together).
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    #[must_use]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x
            && y >= self.y
            && i64::from(x) < i64::from(self.x) + i64::from(self.width)
            && i64::from(y) < i64::from(self.y) + i64::from(self.height)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }
}

/// A top-level window on the desktop.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WindowInfo {
    /// The window's ref for the other tools, such as w1312; it stays while the window exists.
    pub window: String,
    pub title: String,
    /// The program in the catalog it belongs to, when one matches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    pub process_id: u32,
    /// The program file's name, such as notepad.exe.
    pub process_name: String,
    pub rect: Rect,
    pub minimized: bool,
    /// The window the user's input goes to now.
    pub foreground: bool,
}

/// What `computer.window` does to a window, as its title bar would.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowAction {
    /// Brings it to the front, restoring it when it is minimized.
    Front,
    Minimize,
    /// Fills the screen; it comes to the front.
    Maximize,
    /// Back to its normal size, neither minimized nor maximized, without coming to the front.
    Restore,
    /// Asks it to close, as its close button does: the program may ask whether to save first.
    Close,
}

/// Turns a window handle into the ref the tools use, and back.
#[must_use]
pub fn window_ref(handle: isize) -> String {
    format!("w{handle}")
}

#[must_use]
pub fn window_handle(window: &str) -> Option<isize> {
    window.strip_prefix('w')?.parse().ok().filter(|handle| *handle != 0)
}
