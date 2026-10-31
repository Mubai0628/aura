//! Main decodes only the bytes of one Core-verified, authorized read handle.
mod bridge;
mod invoke;
mod read;
mod result;
mod source;
pub use bridge::MediaBridge;
pub(in crate::agent_execution) use invoke::invoke;
pub(in crate::agent_execution) use source::{inspect, resolve};
#[cfg(test)]
mod bridge_tests;
