//! Compile-time isolated P08 contract driver used only by the Electron E2E
//! build. Nothing in this module is registered in the production contract or
//! compiled into the default `aura-core` binary.

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use anyhow::{Context, Result, bail};
use aura_domain_common::{
    AgentRunId, CorrelationId, ErrorCategory, ErrorCode, Revision, RuntimeInstanceId, SessionId,
    TurnTaskId, UtcTimestamp, WorkspaceId,
};
use aura_ipc::JsonRpcRequest;
use aura_network_control::AuthenticatedConfigurationChannelIdentity;
use aura_persistence::PersistenceCoordinator;
use aura_runtime_contract::{IpcErrorEnvelope, IpcRequestTerminal};
use aura_workspace_domain::{CatalogFreshness, SessionSummary, WorkspaceState, WorkspaceSummary};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{RuntimeServices, RuntimeState};

mod workspace_fixture;

pub(crate) const METHOD_V1: &str = "internal.e2e.p08_contract_driver.run_vertical_suite";
const ISOLATED_PROFILE_V1: &str = "p07-isolated-local-fixtures-v1";

#[derive(Clone, Debug)]
pub(crate) struct P08ContractDriverOptionsV1 {
    token: String,
    workspace_fixture_root: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct P08ContractDriverOptionsBuilderV1 {
    profile: Option<String>,
    token: Option<String>,
    workspace_fixture_root: Option<PathBuf>,
}

impl P08ContractDriverOptionsBuilderV1 {
    pub(crate) fn try_parse(&mut self, argument: &str) -> Result<bool> {
        if let Some(value) = argument.strip_prefix("--p08-contract-driver-profile=") {
            self.profile = Some(value.to_owned());
        } else if let Some(value) = argument.strip_prefix("--p08-contract-driver-token=") {
            self.token = Some(value.to_owned());
        } else if let Some(value) =
            argument.strip_prefix("--p08-contract-driver-workspace-fixture-root=")
        {
            self.workspace_fixture_root = Some(PathBuf::from(value));
        } else {
            return Ok(false);
        }
        Ok(true)
    }

    pub(crate) fn finish(self) -> Result<Option<P08ContractDriverOptionsV1>> {
        let configured =
            self.profile.is_some() || self.token.is_some() || self.workspace_fixture_root.is_some();
        if !configured {
            return Ok(None);
        }
        if self.profile.as_deref() != Some(ISOLATED_PROFILE_V1) {
            bail!("the P08 Contract Driver requires the isolated E2E runtime profile");
        }
        let token = self.token.context("missing P08 Contract Driver token")?;
        if token.len() != 64
            || !token.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            bail!("the P08 Contract Driver token must be 256-bit lowercase hexadecimal");
        }
        if self.workspace_fixture_root.as_ref().is_some_and(|root| !root.is_dir()) {
            bail!("the isolated E2E Workspace fixture root is unavailable");
        }
        Ok(Some(P08ContractDriverOptionsV1 {
            token,
            workspace_fixture_root: self.workspace_fixture_root,
        }))
    }
}

pub(crate) fn seed_isolated_workspace_session_v1(
    options: Option<&P08ContractDriverOptionsV1>,
    persistence: &PersistenceCoordinator,
) -> Result<()> {
    let Some(root) = options.and_then(|options| options.workspace_fixture_root.as_ref()) else {
        return Ok(());
    };
    let canonical_root =
        root.canonicalize().context("canonicalize isolated E2E Workspace fixture root")?;
    // A Core restart inside the isolated E2E profile runs this again against the
    // same database. `upsert_workspace_session` would insert a second Workspace with
    // the same canonical root and fail the unique constraint, which used to drop the
    // restarted Core into read-only recovery. Reuse the persisted identities,
    // but restore their projections for this RuntimeInstanceId too.
    if let Some(workspace) = persistence
        .find_workspace_by_root(&canonical_root.display().to_string(), None, None)
        .context("check isolated E2E Workspace fixture")?
    {
        return workspace_fixture::restore_registry(persistence, workspace.workspace_id);
    }
    let workspace_id = WorkspaceId::new();
    let session_id = SessionId::new();
    persistence
        .upsert_workspace_session(
            WorkspaceSummary {
                workspace_id,
                display_name: "P07 Tool Settings Fixture".to_owned(),
                selected_root: root.display().to_string(),
                canonical_root: canonical_root.display().to_string(),
                root_identity: None,
                state: WorkspaceState::Online,
                revision: Revision::INITIAL,
                catalog_freshness: CatalogFreshness::Unavailable,
                authorization_epoch: Revision::INITIAL,
            },
            SessionSummary {
                session_id,
                workspace_id,
                title: "P07 Tool Settings Session".to_owned(),
                state: "idle".to_owned(),
            },
            CorrelationId::new(),
        )
        .context("seed isolated E2E Workspace and Session")?;
    // The Tool settings surface reads the *Session Effective Registry*, and a Session
    // Registry scope is defined by a TurnTask and an AgentRun. The isolated fixture
    // therefore seeds a real, related pair (running, so the scope is current) and the
    // initial Registry head for it — an execution context is never invented for a
    // display, which is why this exists only in the feature-gated E2E driver.
    persistence
        .ensure_session_permission_state(
            session_id,
            workspace_id,
            aura_security_contract::PermissionMode::FullAccess,
            CorrelationId::new(),
        )
        .context("seed isolated E2E Session permission state")?;
    let turn_task_id = TurnTaskId::new();
    let agent_run_id = AgentRunId::new();
    {
        let connection = rusqlite::Connection::open(persistence.database_path())
            .context("open isolated E2E database to seed the execution context")?;
        let now = UtcTimestamp::now().to_string();
        connection
            .execute(
                "INSERT INTO turn_tasks(turn_task_id, session_id, state, created_at, updated_at)
                 VALUES (?1, ?2, 'running', ?3, ?3)",
                rusqlite::params![turn_task_id.to_string(), session_id.to_string(), now],
            )
            .context("seed isolated E2E TurnTask")?;
        connection
            .execute(
                "INSERT INTO agent_runs(
                    agent_run_id, turn_task_id, parent_agent_run_id, state, created_at, updated_at
                 ) VALUES (?1, ?2, NULL, 'running', ?3, ?3)",
                rusqlite::params![agent_run_id.to_string(), turn_task_id.to_string(), now],
            )
            .context("seed isolated E2E AgentRun")?;
    }
    let settings = aura_tool_contract::ToolSettingsSnapshotV1::default();
    let permission = persistence
        .session_permission_state(session_id)
        .context("read isolated E2E Session permission state")?;
    for scope in [
        aura_tool_contract::ToolRegistryScopeV1::Workspace { workspace_id },
        aura_tool_contract::ToolRegistryScopeV1::Session {
            workspace_id,
            session_id,
            turn_task_id,
            agent_run_id,
        },
    ] {
        crate::p07_registry::ToolRegistryControlServiceV1
            .build_scope(
                persistence,
                aura_persistence::ToolRegistryBuildRequestV1 {
                    scope,
                    settings: settings.clone(),
                    mode_permission_context_hash: aura_domain_common::jcs::canonical_sha256(&Some(
                        permission.clone(),
                    ))
                    .context("hash isolated E2E permission context")?,
                    core_exposed_tool_ids: Default::default(),
                    non_package_candidates: Vec::new(),
                    created_at: UtcTimestamp::now(),
                    correlation_id: CorrelationId::new(),
                },
            )
            .context("seed isolated E2E Registry head")?;
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct P08ContractDriverRequestV1 {
    token: String,
    claimed_core_runtime_instance_id: RuntimeInstanceId,
    claimed_main_runtime_instance_id: RuntimeInstanceId,
}

#[derive(Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct P08ContractDriverReportV1 {
    electron_e2e_contract_driver: &'static str,
    fixture_dispatch_count: u8,
    isolated_runtime_profile: &'static str,
    p08_production_agent_loop: &'static str,
    production_p05_to_p08_trigger_count: u8,
}

pub(crate) async fn dispatch_v1(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
    channel: AuthenticatedConfigurationChannelIdentity,
) -> std::result::Result<Value, IpcErrorEnvelope> {
    let envelope =
        |code: &'static str, category: ErrorCategory, message: String| IpcErrorEnvelope {
            code: ErrorCode::new(code).expect("static P08 Contract Driver error code"),
            category,
            message,
            retryable: false,
            correlation_id: request.id,
            terminal: IpcRequestTerminal::Failed,
            details: Default::default(),
        };
    let fail = |code: &'static str, category: ErrorCategory, message: &'static str| {
        envelope(code, category, message.to_owned())
    };
    let fail_detail = |code: &'static str, category: ErrorCategory, message: String| {
        envelope(code, category, message)
    };
    let Some(options) = state.options.p08_contract_driver.as_ref() else {
        return Err(fail(
            "p08.contract_driver.profile_required",
            ErrorCategory::Unavailable,
            "The P08 Contract Driver is unavailable outside its isolated E2E runtime profile.",
        ));
    };
    if request.command_invocation_id.is_none() {
        return Err(fail(
            "p08.contract_driver.identity_required",
            ErrorCategory::Validation,
            "The P08 Contract Driver requires an authenticated command identity.",
        ));
    }
    let input: P08ContractDriverRequestV1 = serde_json::from_value(request.params.clone())
        .map_err(|_| {
            fail(
                "p08.contract_driver.request_invalid",
                ErrorCategory::Validation,
                "The P08 Contract Driver request is invalid.",
            )
        })?;
    if !constant_time_equal(input.token.as_bytes(), options.token.as_bytes())
        || input.claimed_core_runtime_instance_id != channel.core_runtime_instance_id
        || input.claimed_main_runtime_instance_id != channel.authenticated_main_runtime_instance_id
    {
        return Err(fail(
            "p08.contract_driver.identity_rejected",
            ErrorCategory::Permission,
            "The P08 Contract Driver identity or token was rejected.",
        ));
    }
    services.persistence.current_tool_principal().map_err(|_| {
        fail(
            "p08.contract_driver.principal_unavailable",
            ErrorCategory::Unavailable,
            "The isolated E2E principal is unavailable.",
        )
    })?;
    if state.p08_contract_driver_consumed.swap(true, Ordering::AcqRel) {
        return Err(fail(
            "p08.contract_driver.already_consumed",
            ErrorCategory::Conflict,
            "This isolated P08 Contract Driver instance was already consumed.",
        ));
    }

    // The stage index is published before each path starts, so a failure names the
    // fixture that produced it: the previous single task turned every failure into
    // `vertical_suite_failed` with no statement about which of the four paths broke,
    // which is what made the E2E red item undiagnosable.
    // Retired the P07 package command, stdio and HTTP stages with the old package path.
    const STAGES: [&str; 1] = ["builtin"];
    let stage_index = Arc::new(AtomicUsize::new(0));
    let published_index = Arc::clone(&stage_index);
    let suite = tokio::spawn(async move {
        published_index.store(0, Ordering::SeqCst);
        crate::p07_execution::tests::run_p08_builtin_vertical_fixture_v1().await;
    })
    .await;
    if let Err(error) = suite {
        let stage = STAGES[stage_index.load(Ordering::SeqCst).min(STAGES.len() - 1)];
        return Err(fail_detail(
            "p08.contract_driver.vertical_suite_failed",
            ErrorCategory::Internal,
            format!("The P08 Contract Driver vertical fixture suite failed in {stage}: {error}."),
        ));
    }
    serde_json::to_value(P08ContractDriverReportV1 {
        electron_e2e_contract_driver: "passed",
        fixture_dispatch_count: 7,
        isolated_runtime_profile: ISOLATED_PROFILE_V1,
        p08_production_agent_loop: "deferred_to_p08",
        production_p05_to_p08_trigger_count: 0,
    })
    .map_err(|_| {
        fail(
            "p08.contract_driver.report_failed",
            ErrorCategory::Internal,
            "The P08 Contract Driver report could not be encoded.",
        )
    })
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter().zip(right).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
}
