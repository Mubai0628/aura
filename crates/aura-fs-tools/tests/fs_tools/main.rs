//! Integration tests of `aura-fs-tools` in one test program, one module per behaviour.

mod agent_find;
mod agent_write_folders;
mod download_folders;
mod download_publication;
mod external_read;
mod external_search_glob;
mod external_write;
mod file_in_use;
mod ignored_listing;
mod incremental_catalog;
mod listing_bound;
mod p03_fs_tools;
mod read_streaming;
mod retry_fault;
mod search_bench;
mod search_engine;
mod search_growth;
mod search_memory;
mod search_old_baseline;
mod search_perf;
mod unindexed_lookups;
mod vault_atomicity;
mod vault_scan_bounds;
