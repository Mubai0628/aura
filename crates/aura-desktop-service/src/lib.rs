//! Core's side of computer control: the settings in `settings/computer.toml`, the one desktop
//! host (`aura-desktop-host.exe`, started beside Core the first time it is needed, ended when it
//! stops answering and started again for the next request), which programs each session is
//! controlling now, stopping them, the always-allowed list, and the refs the tools hand out.
//! [`child`] is the user's child session, whose host Core reaches through the supervisor there.
//! Whether a call may run is the permission engine's; this crate only carries out what was
//! authorized.
#![forbid(unsafe_code)]

mod calls;
pub mod child;
mod control;
mod launch;
mod link;
pub mod pip;
#[cfg(any(test, feature = "test-hooks"))]
pub mod test_host;

pub use calls::{
    ELEMENT_REFS, FoundApp, KnownElement, KnownShot, Listed, Pinned, SHOT_REFS, WINDOWS_LIMIT,
    ref_number,
};
pub use control::{
    CATALOG_FOR, CATALOG_LIMIT, Caller, Controlled, Controlling, DesktopControl, ICON_SIZE,
    ICONS_LIMIT, ICONS_PER_READ, taken_over_refusal,
};
pub use launch::{HOST_PROGRAM, HostEvent, HostLauncher, HostProcess, Lines, ProgramHost};
pub use link::{HostLink, LinkError, READY_WITHIN, REPLY_MARGIN};
