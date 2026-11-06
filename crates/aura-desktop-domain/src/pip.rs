//! Lines of JSON between Core and the picture-in-picture window of the user's child session,
//! `aura-desktop-host.exe --pip …`, one message per line on the window's standard input and
//! output. The window connects a Remote Desktop control to the child session, where Windows asks
//! the user to sign in; it says what happens, and tells Core when the user takes the child
//! session's desktop over from a turn. Closing it signs the child session out.
use crate::input::{Chord, VK_CONTROL, VK_LWIN, VK_MENU, VK_SHIFT};
use serde::{Deserialize, Serialize};

/// The command line switch that runs the desktop host as the picture-in-picture window.
pub const PIP_SWITCH: &str = "--pip";
/// The protocol both ends speak; the window says it first.
pub const PIP_PROTOCOL_VERSION: u32 = 2;
/// The child session's desktop size when Core gives none.
pub const PIP_DESKTOP: (u32, u32) = (1280, 800);
/// The stop shortcut when the user chose none.
pub const PIP_STOP_KEYS: &str = "Ctrl+Alt+Q";

/// What the window is started with.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipOptions {
    pub width: u32,
    pub height: u32,
    /// The stop shortcut, as [`stop_chord`] reads it.
    pub stop_keys: String,
}

impl Default for PipOptions {
    fn default() -> Self {
        Self { width: PIP_DESKTOP.0, height: PIP_DESKTOP.1, stop_keys: PIP_STOP_KEYS.to_owned() }
    }
}

impl PipOptions {
    /// The arguments after [`PIP_SWITCH`].
    #[must_use]
    pub fn arguments(&self) -> Vec<String> {
        vec![
            "--size".to_owned(),
            format!("{}x{}", self.width, self.height),
            "--stop-keys".to_owned(),
            self.stop_keys.clone(),
        ]
    }

    /// Reads the arguments after [`PIP_SWITCH`].
    ///
    /// # Errors
    ///
    /// An argument that is not one of these, or a size outside what Remote Desktop takes.
    pub fn parse(arguments: &[String]) -> Result<Self, String> {
        let mut options = Self::default();
        let mut seen = std::collections::BTreeSet::new();
        let mut rest = arguments.iter();
        while let Some(name) = rest.next() {
            if !seen.insert(name) {
                return Err(format!("参数重复：{name}"));
            }
            let value = rest.next().ok_or_else(|| format!("{name} 后面少了值"))?;
            match name.as_str() {
                "--size" => {
                    let (width, height) = value
                        .split_once('x')
                        .and_then(|(width, height)| {
                            Some((width.parse().ok()?, height.parse().ok()?))
                        })
                        .ok_or_else(|| format!("大小写法不对：{value}"))?;
                    if !(640..=3840).contains(&width) || !(480..=2160).contains(&height) {
                        return Err(format!("大小超出范围：{value}"));
                    }
                    (options.width, options.height) = (width, height);
                }
                "--stop-keys" => {
                    stop_chord(value)?;
                    options.stop_keys.clone_from(value);
                }
                other => return Err(format!("不认识的参数：{other}")),
            }
        }
        Ok(options)
    }
}

/// Reads a stop shortcut: an ordinary key held with Control or Alt, and no chord that Windows
/// answers itself. It is registered for the whole desktop while the window is open.
///
/// # Errors
///
/// A chord [`crate::parse_chord`] does not read, one without Control or Alt, one that ends in a
/// modifier, or a system chord.
pub fn stop_chord(keys: &str) -> Result<Chord, String> {
    let chord = crate::parse_chord(keys)?;
    let held = |code: u16| chord.modifiers.iter().any(|key| key.code == code);
    if matches!(chord.key.code, VK_SHIFT | VK_CONTROL | VK_MENU | VK_LWIN) {
        return Err(format!("停止快捷键的最后一个要是普通键：{keys}"));
    }
    if !(held(VK_CONTROL) || held(VK_MENU)) {
        return Err(format!("停止快捷键要带 Ctrl 或 Alt：{keys}"));
    }
    if chord.is_system() {
        return Err(format!("{keys} 是系统快捷键，不能用来停止"));
    }
    Ok(chord)
}

/// What Core tells the window.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipCommand {
    /// A turn keeps the child session's desktop, or no longer does. While one does, the window
    /// passes no input to the child session; a click into it, the stop shortcut or 停止 takes the
    /// desktop over. Otherwise the user can work in the child session freely.
    Hold {
        on: bool,
        request: u64,
    },
    /// Signs the child session out and closes the window.
    Close,
    StopKeys {
        keys: String,
    },
}

/// What the window says.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipMessage {
    /// First and once.
    Ready {
        protocol: u32,
    },
    /// The control is connecting; Windows asks the user to sign in, or signs them in.
    Connecting,
    /// The child session is signed in.
    SignedIn {
        session: u32,
    },
    /// The connection ended without the user closing the window: Windows' reason code. The child
    /// session stays as it is; the window offers to connect again.
    Disconnected {
        reason: i32,
    },
    /// During a hold the user clicked into the window, pressed the stop shortcut or 停止.
    TakenOver,
    /// Input gating and takeover observation have been applied before this acknowledgement.
    Holding {
        on: bool,
        request: u64,
    },
    /// The window cannot safely connect or gate input; no desktop action was performed.
    Failed {
        message: String,
    },
    /// The stop shortcut could not be registered: another program uses it.
    StopKeysTaken {
        keys: String,
    },
    StopKeysReady,
    /// The window closed: the user closed it, Core told it to, or the user kept a child session
    /// from before rather than sign it out. Whether the child session was signed out.
    Closed {
        signed_out: bool,
    },
}
