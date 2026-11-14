//! Integration tests of `aura-workspace` in one test program, one module per behaviour.

mod archive_while_indexing;
mod catalog_concurrency;
mod catalog_incremental;
mod catalog_reuse;
mod index_correctness;
mod lifecycle_failure_log;
mod open_isolation;
mod open_report;
mod p03_workspace;
mod real_root_probe;
mod startup_cost_probe;
mod storage_isolation;
mod watcher_convergence;
mod worktree_workspaces;

mod runtime_support;
