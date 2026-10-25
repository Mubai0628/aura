//! The Aura setup program: installs, upgrades and repairs an installation from the release it
//! carries, and uninstalls one. Everything happens for the current user, without administrator
//! rights.
#![forbid(unsafe_code)]

pub mod host;
pub mod install;
pub mod pack;
pub mod payload;
pub mod uninstall;

#[cfg(test)]
mod tests;
