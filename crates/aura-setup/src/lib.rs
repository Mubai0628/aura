//! The Aura setup program: installs, upgrades and repairs an installation from the release it
//! carries, and uninstalls one. Everything happens for the current user, without administrator
//! rights.
#![forbid(unsafe_code)]

pub mod data_choice;
pub mod folder_check;
pub mod host;
pub mod install;
pub mod marker;
pub mod pack;
pub mod payload;
pub mod private_folder;
pub mod register;
pub mod relocate;
pub mod uninstall;
pub mod wizard;

#[cfg(test)]
mod tests;
