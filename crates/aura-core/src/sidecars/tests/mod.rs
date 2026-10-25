use std::os::windows::ffi::OsStrExt;

use aura_domain_common::{
    CancellationId, ContentHashAlgorithm, PermissionSnapshotId, Revision, SandboxExecutionId,
    SessionId, WorkspaceId,
};
use aura_permission_domain::{
    ExternalIsolationMode, NetworkAccessRequirement, ProcessOutputLimits, SecuritySource,
    WindowsOsString,
};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_sandbox::{external_process_plan_hash, snapshot_executable_ref};
use aura_security_contract::TrustState;
use aura_security_contract::{CanonicalPath, InvocationIdentity};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use rusqlite::Connection;
use tokio::task::JoinHandle;

use super::*;

// Behavior tests are real child modules; shared fixtures are imported explicitly.
mod authentication;
mod command;
mod external_process;
mod stdio;

use super::authentication::constant_time_equal;
use super::channel::{PendingRequest, handle_inbound, write_sidecar_request};
use super::command::{
    accept_command_host_notification, command_candidate_contains_bytes,
    reconcile_command_dispatch_candidate,
};
use super::stdio::{
    StdioDispatchStreamErrorV1, reconcile_stdio_dispatch_candidate, stdio_candidate_contains_bytes,
};
