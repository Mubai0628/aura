#![forbid(unsafe_code)]
//! The user's own Chrome, shared into an Aura session. `browser.*` keeps Aura's built-in
//! browser; nothing here is shared with it, and nothing here is Aura's own network.
pub mod commands;
pub mod control;
pub mod protocol;
pub mod tools;

pub use control::*;
pub use tools::*;
