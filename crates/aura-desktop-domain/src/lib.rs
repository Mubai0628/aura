#![forbid(unsafe_code)]
//! Computer use: the programs on the user's own desktop that the user lets Aura operate. Core
//! asks Aura's desktop host for what it needs over [`protocol`]; the host is the only process that
//! reads windows, takes screenshots or sends input.
pub mod app;
pub mod child;
pub mod element;
pub mod input;
mod mode;
pub mod pip;
pub use mode::{ComputerMode, ComputerPipStatus, PipPhase};
pub mod protocol;
pub mod safety;
pub mod screen;
pub mod tools;

pub use app::*;
pub use element::*;
pub use input::{Chord, InputStep, Key, MouseButton, Point, parse_chord};
pub use protocol::*;
pub use screen::*;
pub use tools::*;
