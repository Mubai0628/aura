//! Agent execution in Core: the Agent's tool calls from admission to completion, command and
//! terminal execution, the controlled web, the dispatch reservation, and the execution's
//! activity publication.
#![forbid(unsafe_code)]

/// The projections a real Agent executor publishes into the run-information
/// index, and the query surface over that index.
pub mod agent_activity;
pub mod agent_execution;
pub mod broker;
pub mod command_execution;
pub mod controlled_web;
pub mod security_control;
