#![forbid(unsafe_code)]
//! Core-side controller and isolated Runner implementation share only native
//! execution resources. Neither this crate nor the Runner opens a database.
pub mod checked_program;
pub mod compat;
mod controller;
pub mod environment;
pub mod file_binding;
pub mod file_lease;
pub mod git;
pub mod installation;
pub mod lease_recovery;
pub mod maintenance;
mod managed_operations;
mod native;
mod native_execute;
mod observation;
pub mod program;
pub mod runner;
mod runner_execution;
mod runtime;
pub mod runtime_package;
mod supervisor;
mod wire;
mod work;
pub use controller::{
    LaunchFuture, LaunchLease, LaunchStage, LaunchStageKind, NotStarted, PortFuture, PortSection,
    ScopeFacts, WindowsUserBackend, WindowsUserCorePort, WindowsUserCorePortSection,
};
pub use native::{NATIVE_BACKEND_ID, NativeApprovedBackend};
pub use observation::ShellExecutionObserver;
pub use work::{AncestorWork, GrantWork, LaunchWorkV1};
#[cfg(test)]
mod checked_program_tests;
#[cfg(test)]
mod controller_tests;
#[cfg(test)]
mod file_binding_tests;
#[cfg(test)]
mod file_lease_delete_tests;
#[cfg(test)]
mod file_lease_partial_tests;
#[cfg(test)]
mod file_lease_tests;
#[cfg(test)]
mod installation_tests;
#[cfg(test)]
mod lease_recovery_tests;
#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod runtime_tests;
