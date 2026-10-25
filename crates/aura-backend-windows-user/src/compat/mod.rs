//! Current-user managed Shell with an owned non-breakaway Job.
mod backend;
mod background;
mod control;
pub mod environment;
mod execution;
mod job_owner;
mod monitor;
mod output;
mod preparation;
mod registry;
mod runtime;
mod service;
pub(crate) mod soft_proxy;
mod terminal;
#[cfg(feature = "terminal-test-profile")]
mod terminal_opening_gate;
pub use service::{COMPAT_BACKEND_ID, CompatExecutionBackend};
pub use terminal::{
    TerminalEnd, TerminalEndReason, TerminalKind, TerminalOptions, TerminalOutputPage,
    TerminalRegistry, TerminalSession, TerminalSummary,
};

pub use backend::CompatBackend;
#[cfg(any(test, feature = "compat-adoption"))]
pub use backend::LaunchFault;
pub use job_owner::{CompatExecution, DirectResult, JobObservation, JobReceipt, StopObservation};
pub use registry::CompatJobs;
