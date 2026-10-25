//! Independent, disposable per-Vault indexes. Core supplies a cache path outside
//! the Vault. No project discovery or cross-Vault query exists in this module,
//! and none of these tables is part of the shared V2 database.
mod history;
mod index;
mod maintenance;
mod search;
pub use index::{CACHE_SCHEMA_VERSION, IndexedFile, MemoryIndex, VaultIndexStatus};
pub use search::{MemorySearch, MemorySearchHit, MemorySearchResult};
