//! p03's tests that need a started Core: the request lanes' waits on a held workspace resource.
//! They read p03's names through `use super::*`, as they did inside its module; these imports
//! give them the same names, p03's items and the ones p03 itself imported.
use aura_domain_common::{CorrelationId, ErrorCategory};
use aura_fs_tools::{FsToolPolicy, FsToolService};
use aura_ipc::method;
use aura_workspace_domain::FsWriteFileRequest;
use aura_workspace_service::p03::*;
use serde_json::Value;
use std::{sync::Arc, time::Duration, time::Instant};

#[cfg(test)]
#[path = "p03_resource_wait_tests.rs"]
mod resource_wait_tests;
