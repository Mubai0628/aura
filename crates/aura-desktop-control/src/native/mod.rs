//! The crate's boundary with Windows. Each function here does one thing with the system and
//! returns owned Rust values; handles and COM pointers never leave this module, except inside
//! values the rest of the crate can only hand back to it (an element of `uia`).
pub mod apps;
mod com;
pub mod dpi;
pub mod focus;
pub mod guard;
pub mod icon;
pub mod image;
pub mod input;
pub mod launch;
pub mod process;
pub mod screen;
mod text;
pub mod uia;
pub mod window_list;
pub mod window_state;

pub use com::Com;
