//! Mode two of computer use: Aura operates the programs of the user's Windows child session, a
//! second session of the same user that a small window shows, so that the user's own pointer and
//! keyboard are left alone. Aura's desktop host program takes the roles of this crate by its
//! command line:
//! - [`child`]: the supervisor in the child session, which connects back to Core's pipe and runs
//!   the desktop host of the child session on Core's orders.
//! - [`pip`]: the picture-in-picture window in the user's own session, which shows the child
//!   session, where the user signs in and can take its desktop over from a turn.
#![deny(unsafe_code)]

pub mod child;
mod lines;
#[allow(unsafe_code)]
mod native;
pub mod pip;
