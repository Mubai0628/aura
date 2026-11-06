//! Simulated mouse and keyboard input, as the host carries it out: points are screen points
//! (Core turns screenshot points into them), keys are named the way the tools name them.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton {
    #[default]
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputStep {
    Click {
        at: Point,
        #[serde(default)]
        button: MouseButton,
        /// 2 is a double click.
        #[serde(default = "one")]
        count: u8,
    },
    Move {
        at: Point,
    },
    /// Presses at the first point, moves through the others and lets go at the last.
    Drag {
        path: Vec<Point>,
        #[serde(default)]
        button: MouseButton,
    },
    /// Turns the wheel over a point: positive `down` scrolls down, positive `right` right; one
    /// unit is one notch.
    Scroll {
        at: Point,
        #[serde(default)]
        down: i32,
        #[serde(default)]
        right: i32,
    },
    Type {
        text: String,
    },
    Press {
        keys: String,
        #[serde(default = "one")]
        repeat: u8,
    },
    Wait {
        ms: u32,
    },
}

fn one() -> u8 {
    1
}

/// One key of a chord, as Windows numbers it (a virtual-key code).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Key {
    pub code: u16,
    /// Arrows, Home, End, Insert, Delete, Page Up and Page Down are extended keys.
    pub extended: bool,
}

pub const VK_SHIFT: u16 = 0x10;
pub const VK_CONTROL: u16 = 0x11;
pub const VK_MENU: u16 = 0x12;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_TAB: u16 = 0x09;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_DELETE: u16 = 0x2E;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_SNAPSHOT: u16 = 0x2C;

/// A chord such as `Control+Shift+s`, `Enter` or `Alt+F4`: modifiers first, the key last.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chord {
    pub modifiers: Vec<Key>,
    pub key: Key,
}

impl Chord {
    /// Whether the chord is one the system answers instead of the program: anything with the
    /// Windows key, switching programs (Alt+Tab, Alt+Esc), the Start menu (Ctrl+Esc), Task Manager
    /// (Ctrl+Shift+Esc), Ctrl+Alt+Delete and Print Screen (which opens the Snipping Tool).
    #[must_use]
    pub fn is_system(&self) -> bool {
        let held = |code: u16| self.modifiers.iter().any(|key| key.code == code);
        let (control, alt) = (held(VK_CONTROL), held(VK_MENU));
        held(VK_LWIN)
            || self.key.code == VK_LWIN
            || (alt && matches!(self.key.code, VK_TAB | VK_ESCAPE))
            || (control && self.key.code == VK_ESCAPE)
            || (control && alt && self.key.code == VK_DELETE)
            || self.key.code == VK_SNAPSHOT
    }
}

/// Reads `keys`; letters and digits are the keys that type them on a US layout.
pub fn parse_chord(keys: &str) -> Result<Chord, String> {
    let parts: Vec<&str> = keys.split('+').map(str::trim).collect();
    if keys.trim().is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(format!("按键写法不对：{keys:?}，例如 Control+s、Enter、Alt+F4"));
    }
    let mut all = parts
        .iter()
        .map(|part| key(part).ok_or_else(|| format!("不认识的键：{part}")))
        .collect::<Result<Vec<Key>, String>>()?;
    let key = all.pop().ok_or_else(|| format!("按键写法不对：{keys:?}"))?;
    for modifier in &all {
        if !matches!(modifier.code, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN) {
            return Err(format!(
                "{keys:?} 里只有最后一个能是普通键，前面的要是 Control、Shift、Alt 或 Win"
            ));
        }
    }
    Ok(Chord { modifiers: all, key })
}

fn key(name: &str) -> Option<Key> {
    let plain = |code: u16| Some(Key { code, extended: false });
    let extended = |code: u16| Some(Key { code, extended: true });
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "control" | "ctrl" => plain(VK_CONTROL),
        "shift" => plain(VK_SHIFT),
        "alt" | "option" => plain(VK_MENU),
        "win" | "windows" | "meta" | "super" | "cmd" => extended(VK_LWIN),
        "enter" | "return" => plain(VK_RETURN),
        "tab" => plain(VK_TAB),
        "escape" | "esc" => plain(VK_ESCAPE),
        "backspace" => plain(0x08),
        "space" => plain(0x20),
        "delete" | "del" => extended(VK_DELETE),
        "insert" | "ins" => extended(0x2D),
        "home" => extended(0x24),
        "end" => extended(0x23),
        "pageup" => extended(0x21),
        "pagedown" => extended(0x22),
        "arrowleft" | "left" => extended(0x25),
        "arrowup" | "up" => extended(0x26),
        "arrowright" | "right" => extended(0x27),
        "arrowdown" | "down" => extended(0x28),
        "capslock" => plain(0x14),
        "printscreen" => extended(VK_SNAPSHOT),
        "contextmenu" | "menu" | "apps" => extended(0x5D),
        _ => function_key(&lower).or_else(|| character(name)),
    }
}

fn function_key(lower: &str) -> Option<Key> {
    let number: u16 = lower.strip_prefix('f')?.parse().ok()?;
    (1..=24).contains(&number).then_some(Key { code: 0x70 + number - 1, extended: false })
}

fn character(name: &str) -> Option<Key> {
    let mut chars = name.chars();
    let (Some(only), None) = (chars.next(), chars.next()) else { return None };
    let code = match only {
        'a'..='z' => only.to_ascii_uppercase() as u16,
        'A'..='Z' | '0'..='9' => only as u16,
        ';' | ':' => 0xBA,
        '=' => 0xBB,
        ',' => 0xBC,
        '-' => 0xBD,
        '.' => 0xBE,
        '/' | '?' => 0xBF,
        '`' | '~' => 0xC0,
        '[' | '{' => 0xDB,
        '\\' | '|' => 0xDC,
        ']' | '}' => 0xDD,
        '\'' | '"' => 0xDE,
        _ => return None,
    };
    Some(Key { code, extended: false })
}
