//! Tools in Core: the tool registry and its controls, built-in tools and tool commands, the
//! sandbox and output contracts of a tool call, and the tool credential service.
#![forbid(unsafe_code)]

#[allow(
    dead_code,
    reason = "only the module's tests use the remaining Sandbox launch-descriptor check"
)]
pub mod p07;
pub mod p07_builtin;
pub mod p07_commands;
pub mod p07_control;
#[allow(dead_code, reason = "only the module's tests use the remaining output verification")]
pub mod p07_output;
#[allow(
    dead_code,
    reason = "the exact Registry control service is ready before the P08 Core Tool route"
)]
pub mod p07_registry;
pub mod tool_credential_service;
