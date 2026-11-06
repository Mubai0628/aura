//! Aura's desktop host: the one process that reads the user's programs and windows for computer
//! use. Core starts it and speaks [`aura_desktop_domain::protocol`] with it over its standard
//! input and output.
//!
//! Windows is reached only from [`native`], the crate's audited boundary: every unsafe call is
//! there, wrapped in safe functions with a single purpose. The rules (how programs are merged and
//! matched to windows, what a screenshot may show, how input steps are checked, which elements a
//! read lists) and the request loop are safe code that tests run without the desktop.
#[cfg(not(windows))]
compile_error!("aura-desktop-control supports Windows only");

pub mod alpha;
pub mod area;
pub mod arrange;
pub mod banner;
pub mod catalog;
pub mod desktop;
pub mod drag;
pub mod elements;
pub mod front;
pub mod glide;
pub mod guard;
pub mod hold;
pub mod host;
pub mod marks;
pub mod native;
pub mod own;
pub mod plan;
pub mod pointer_marks;
pub mod popup;
pub mod redact;
