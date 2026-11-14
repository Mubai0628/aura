//! Integration tests of `aura-persistence` in one test program, one module per behaviour.

mod deletion_scans;
mod full_index_lane;
mod l13_3_attachment_grants;
mod p02_persistence;
mod p03_workspace_lifecycle;
mod p04_security;
mod p05_model;
mod p05_model_runtime;
mod p06_context;
mod p07_tool_builtin_release;
mod p07_tool_execution;
mod p07_tool_pins;
mod p07_tool_projection;
mod package_tool_credentials;
mod permission_async_isolation;
mod permission_transition;
mod s09_migration;
mod session_metadata;
mod stale_workspace_index;
mod storage_lifecycle;
mod store_slimming;
mod whole_index_read;
mod workspace_index_batches;
mod workspace_index_workload;
mod worktree_links;

mod index_support;
