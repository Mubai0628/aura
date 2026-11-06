//! Core's side of the user's child session (mode two of computer use): one opening at a time.
//!
//! An opening has a pipe name and a token of its own ([`ChildOpening`]). The startup entry Core
//! writes for it runs `aura-desktop-host.exe --child <pipe> <token>` when the child session signs
//! in. [`listen`] makes the pipe before that entry is written; [`Listening::accept`] waits for the
//! supervisor and lets it in only when it is the host program beside Core, runs in the child
//! session, and says the opening's token. The connected supervisor is the launcher of the child
//! session's host ([`ChildHost`]): a [`crate::HostLink`] over it speaks the same protocol, with the
//! same limits, as the one over the host of the user's own desktop.
mod accept;
mod host;
mod opening;

pub use accept::{Expected, HELLO_WITHIN, Listening, Supervisor, listen};
pub use host::ChildHost;
pub use opening::ChildOpening;
