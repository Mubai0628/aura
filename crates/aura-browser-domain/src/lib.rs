#![forbid(unsafe_code)]
//! Aura's built-in browser. The site rules the user sets, the tools the model calls and the
//! commands Core sends the browser host. `web.*` keeps its own types; nothing here is shared with
//! it.
pub mod address;
pub mod host;
pub mod site;
pub mod tools;

pub use address::*;
pub use host::*;
pub use site::*;
pub use tools::*;
