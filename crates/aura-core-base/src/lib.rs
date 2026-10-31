//! What the services inside the Core process share: Core's diagnostics log, the bounded blocking
//! pool and synchronous boundary, disk pressure and busy records, configuration degradation,
//! credential reads, the command ledger and settlement, outbound watch and JSON frames onto the
//! outbound queue, the per-connection request lanes, Core's release identity, the backends'
//! readiness store, the startup guard, the Recycle Bin, a web address's network origin and,
//! behind its feature, the acceptance barrier.
#![forbid(unsafe_code)]

/// The default-off S02-E acceptance boundary. Compiled only with `--features acceptance-barrier`;
/// no product build enables it, and with the feature off this module does not exist at all.
#[cfg(feature = "acceptance-barrier")]
pub mod acceptance_barrier;
pub mod blocking_pool;
pub mod busy;
pub mod command_claims;
pub mod command_settlement;
/// What a configuration fault costs Core, and what it does not.
pub mod config_degradation;
pub mod core_log;
pub mod credential_io;
pub mod disk_pressure;
pub mod envelope;
pub mod ipc_session;
pub mod outbound_json;
pub mod outbound_watch;
pub mod readiness;
pub mod recycle;
pub mod release;
pub mod startup_recovery;
pub mod sync_boundary;
pub mod web_origin;

pub use core_log::append_core_log;

// `core_log` reads these through `use super::*`, as it read them from aura-core's root. Its
// test-hook build has no use for `env`: a test never writes under `LOCALAPPDATA`.
use aura_domain_common::UtcTimestamp;
use serde_json::{Value, json};
#[cfg(not(any(test, feature = "test-hooks")))]
use std::env;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Instant,
};
