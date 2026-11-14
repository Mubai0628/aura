//! The names Aura writes in each of its folders. A folder holding any other name is not taken for
//! Aura's: another program may use it too, or the user keeps something there.
use aura_data_location::{MARKER_FILE, RECORD_FILE, UI_PROFILE_FOLDER};

/// The top of the local Aura folder in the versions before namespaces.
pub(crate) const EARLY_LOCAL: &[&str] = &[
    "config-history",
    "data",
    "desktop",
    "e2e-runs",
    "launcher",
    "plugins",
    "runtime",
    "skills",
    "tools",
    "web",
    "worktrees",
];

/// A settings folder: the top of the roaming Aura folder before namespaces, and each namespace's
/// folder under it since.
pub(crate) const SETTINGS: &[&str] = &[
    "settings",
    "aura.toml",
    "commands.json",
    "hooks.toml",
    "mcp.json",
    "models.json",
    "plugins.json",
    "skills.json",
    "tools.json",
];

/// What every version since namespaces keeps at the top of the local Aura folder: the namespaces
/// and the Core log of every installation at the default places. Never found.
pub(crate) const CURRENT_LOCAL: &[&str] = &["logs", "namespaces"];
pub(crate) const CURRENT_ROAMING: &[&str] = &["namespaces"];

/// A namespace's folder in the default place. A move's journal is not among them: a namespace
/// with a move waiting or under way is not found.
pub(crate) const NAMESPACE: &[&str] = &[
    "backups",
    "chrome",
    "config-history",
    "data",
    "desktop",
    "e2e-runs",
    "launcher",
    "logs",
    "plugins",
    "runtime",
    "skills",
    "tools",
    "updates",
    "web",
    "worktrees",
    UI_PROFILE_FOLDER,
    RECORD_FILE,
];

/// A folder a namespace's data was moved to.
pub(crate) const MOVED: &[&str] = &[MARKER_FILE, "data", "logs", UI_PROFILE_FOLDER];

/// Where Aura's own work used to put working copies: a folder holding any is never found.
pub(crate) const WORKTREES: &str = "worktrees";

/// The end of the name a folder takes once its removal has begun.
pub const REMOVING_SUFFIX: &str = ".aura-removing";

pub(crate) fn is_one_of(name: &str, known: &[&str]) -> bool {
    known.iter().any(|known| known.eq_ignore_ascii_case(name))
}

pub(crate) fn is_leftover(name: &str) -> bool {
    name.len() > REMOVING_SUFFIX.len()
        && name
            .get(name.len() - REMOVING_SUFFIX.len()..)
            .is_some_and(|end| end.eq_ignore_ascii_case(REMOVING_SUFFIX))
}

/// A namespace's folder name: 32 lowercase hexadecimal digits.
pub(crate) fn is_namespace_hash(name: &str) -> bool {
    name.len() == 32 && name.bytes().all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}
