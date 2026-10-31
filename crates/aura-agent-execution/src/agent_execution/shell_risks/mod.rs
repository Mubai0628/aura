//! What a Shell script does that the permission mode decides apart from running it: deleting
//! files (`deletion`, asked every time), downloading (`download`, asked once per session) and
//! writing outside the workspace (`outside`, asked every time) — all below 完全访问, and all read
//! from the script's text.
mod deletion;
mod download;
mod outside;

use aura_windows_user_contract::ManagedShell;
use std::path::{Path, PathBuf};

pub(crate) struct ShellRisks {
    pub deletes: bool,
    pub downloads: bool,
    /// Places outside the workspace the script writes, as the script names them.
    pub outside: Vec<String>,
}

/// `directory` is where the script starts, when it is known; `roots` are the workspace's.
pub(crate) fn of_script(
    shell: ManagedShell,
    script: &str,
    directory: Option<&Path>,
    roots: &[PathBuf],
) -> ShellRisks {
    ShellRisks {
        deletes: deletion::deletes_files(shell, script),
        downloads: download::downloads(script),
        outside: outside::targets(script, directory, roots),
    }
}
