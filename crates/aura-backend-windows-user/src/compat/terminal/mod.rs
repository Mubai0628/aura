//! Process-local terminal lifetime, separate from a one-shot Shell invocation.
mod launch;
mod output;
mod registry;
mod session;
#[cfg(test)]
mod tests;
use aura_windows_platform::TerminalSize;
use aura_windows_user_contract::ManagedShell;
pub use aura_windows_user_contract::{TerminalEnd, TerminalEndReason};
pub use output::TerminalOutputPage;
pub use registry::{TerminalRegistry, TerminalSummary};
pub use session::TerminalSession;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKind {
    User,
    Agent { requires_network: bool },
}
#[derive(Clone, Debug)]
pub struct TerminalOptions {
    pub shell: ManagedShell,
    pub kind: TerminalKind,
    pub working_directory: PathBuf,
    pub size: TerminalSize,
    pub output_limit: usize,
}
