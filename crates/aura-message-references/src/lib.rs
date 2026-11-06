#![forbid(unsafe_code)]
//! What a user message refers to with "@", resolved once when the message is sent: the paths
//! typed after "@" in its words and the references chosen beside them, another conversation with
//! a short recap. Core gives the session's sources; the result is kept with the message and
//! written after its text.
mod recap;
mod resolve;
mod typed;
pub use recap::*;
pub use resolve::*;
