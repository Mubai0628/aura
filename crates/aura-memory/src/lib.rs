#![forbid(unsafe_code)]

//! File-authoritative, independent Vaults. All roots are explicit constructor
//! arguments. No environment lookup, profile fallback or global singleton.
pub use aura_memory_domain::*;
mod audit;
mod desktop;
mod error;
mod events;
mod export;
mod forget;
mod index;
mod journal;
mod maintenance;
mod maintenance_log;
mod maintenance_runs;
mod repair;
mod roots;
mod service;
mod tools;
mod vault;
mod watcher;
mod writer;
pub use aura_persistence_v2::memory::{
    MemorySearch, MemorySearchHit, MemorySearchResult, VaultIndexStatus,
};
pub use error::{MemoryError, MemoryResult};
pub use events::ProjectEvent;
pub use maintenance::validate_maintenance_output;
pub use roots::VaultRoots;
pub use service::{MemoryService, MemoryWorkspace};
#[cfg(test)]
mod tests;
pub use vault::{MemoryRead, Vault};
pub use watcher::VaultWatcher;
pub use writer::{MemoryWrite, WriteActor};
