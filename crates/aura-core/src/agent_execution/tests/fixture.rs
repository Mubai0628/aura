//! Fixtures replace only the Provider output. Calls, admission, policy,
//! execution, file effects, receipts and history use the production services.
mod l02_sensitive;
mod l02_takeover_input;
mod l02_terminal;
mod l03_terminal_wait;
mod l13_terminal_handback;
mod mcp;
pub use mcp::mcp_server_config;
mod packages;
mod sessions;
pub use packages::Packages;
mod steps;
use super::*;
use aura_agent_domain::*;
use aura_persistence_v2::{
    agent::{checkpoint, lifecycle, turns},
    context, model, tool, workspace,
};
use aura_tool_registry::{ParentRegistryBuildInputV1, build_parent_registry_snapshot};

pub struct Fixture {
    pub runtime: Arc<crate::RuntimeState>,
    pub _host: Arc<crate::r03_tests::Fixture>,
    pub root: std::path::PathBuf,
    pub turn: TurnTask,
    pub step: ModelStepId,
    pub settings: ToolSettingsSnapshotV1,
    bundle: aura_domain_common::PromptBundleSnapshotId,
}
impl Fixture {
    pub async fn new(mode: PermissionMode) -> Self {
        Self::with_exposure(
            mode,
            &[
                "fs.write",
                "task_lineage.review_completion",
                "normal.complete",
                "tools.search",
                "tools.describe",
                "tools.load",
                "tools.unload",
                "tools.list_loaded",
            ],
        )
        .await
    }
    pub async fn with_exposure(mode: PermissionMode, names: &[&str]) -> Self {
        Self::with_exposure_setup(mode, names, |_| {}).await
    }
    pub async fn with_exposure_setup(
        mode: PermissionMode,
        names: &[&str],
        setup: impl FnOnce(&std::path::Path),
    ) -> Self {
        let host = Arc::new(crate::r03_tests::Fixture::new());
        let runtime = Arc::new(host.start().await);
        let root = host.root.path().join("agent-workspace");
        std::fs::create_dir(&root).expect("Agent test setup and persisted assertion");
        setup(&root);
        let value = crate::r03_tests::success(
            &runtime,
            "workspace.create",
            serde_json::json!({"selected_root":root,"display_name":"Agent"}),
        )
        .await;
        let workspace: aura_workspace_domain::WorkspaceSummary =
            serde_json::from_value(value["workspace"].clone())
                .expect("Agent test setup and persisted assertion");
        Self::create_in_workspace(host, runtime, root, workspace, mode, names, Packages::None)
    }
    pub fn service(&self) -> &crate::RuntimeServices {
        services(&self.runtime)
    }

    /// The Workspace scope this fixture's committed registry head belongs to.
    ///
    /// The live-enablement port takes a scope, and the scope has to come
    /// from the fixture's own real committed snapshot rather than one the test
    /// invented — that is what makes "the caller cannot forge the scope" true in
    /// the test as well as in production.
    pub fn workspace_scope(&self) -> aura_tool_contract::ToolRegistryScopeV1 {
        self.registry().scope
    }

    /// The settings facade the runtime really owns.
    pub fn settings_facade(&self) -> &std::sync::Arc<aura_config::SettingsFacade> {
        &services(&self.runtime)._settings
    }
    pub fn registry(&self) -> ToolRegistrySnapshotV1 {
        let store = self.service().broker.lock().expect("Agent test setup and persisted assertion");
        let boundary = checkpoint::load(store.database.connection(), self.step)
            .expect("Agent test setup and persisted assertion");
        tool::registry(
            store.database.connection(),
            boundary.registry_snapshot_ref.tool_registry_snapshot_id,
        )
        .expect("Agent test setup and persisted assertion")
        .expect("Agent test setup and persisted assertion")
        .as_ref()
        .clone()
    }
    pub fn output(&self, id: ToolCallId) -> serde_json::Value {
        let store = self.service().broker.lock().expect("Agent test setup and persisted assertion");
        let call = tool::execution::call(store.database.connection(), id)
            .expect("Agent test setup and persisted assertion");
        assert!(
            matches!(
                call.result.as_ref().expect("Agent test setup and persisted assertion").outcome,
                ToolResultOutcomeV1::Succeeded
            ),
            "{:?}",
            call.result
        );
        tool::result::execution_output(store.database.connection(), id)
            .expect("Agent test setup and persisted assertion")
            .1
    }
    pub fn admit(&self, calls: &[(&str, serde_json::Value)]) -> AdmittedBatch {
        let mut output = aura_contract_schema::canonical_fixture()
            .expect("Agent test setup and persisted assertion")
            .models
            .output
            .expect("Agent test setup and persisted assertion");
        output.model_step_id = self.step;
        output.tool_calls = calls
            .iter()
            .enumerate()
            .map(|(i, (name, args))| aura_model_domain::NormalizedToolCall {
                tool_call_id: ToolCallId::new(),
                provider_call_id: format!("actual_provider_call_{i}")
                    .parse()
                    .expect("Agent test setup and persisted assertion"),
                tool_name: aura_model_domain::provider_wire_tool_name(name, self.tool_id(name)),
                position: i as u32,
                arguments_json: steps::flat_arguments(args),
            })
            .collect();
        let batch_id = ToolBatchId::new();
        self.service()
            .broker
            .lock()
            .expect("Agent test setup and persisted assertion")
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::agent::history::record_output(
                    tx,
                    &output,
                    Some(batch_id),
                    UtcTimestamp::now(),
                )?;
                let old = turns::load_run(tx, self.turn.agent_run_id)?;
                let mut next = old.clone();
                next.revision = Revision::new(old.revision.value() + 1);
                next.state = AgentRunState::ToolsRunning { tool_batch_id: batch_id };
                turns::save_run(tx, &old, &next)
            })
            .expect("Agent test setup and persisted assertion");
        self.service()
            .agent_execution_required()
            .admit_output_sync(self.step, self.settings.clone())
            .expect("Agent test setup and persisted assertion")
    }
    /// Admits a second contending batch and reports an **admission refusal as a value**.
    ///
    /// The ordinary `admit` panics on a conflict, which is right for setup. This variant exists
    /// for the regressions that assert a protection still holds: refusing a stale original-content
    /// condition is the correct product behaviour, so the helper must be able to carry that
    /// outcome rather than forbid it.
    pub fn admit_contending(
        &self,
        calls: &[(&str, serde_json::Value)],
    ) -> Result<AdmittedBatch, String> {
        let mut output = aura_contract_schema::canonical_fixture()
            .expect("Agent test setup and persisted assertion")
            .models
            .output
            .expect("Agent test setup and persisted assertion");
        output.model_step_id = self.step;
        output.tool_calls = calls
            .iter()
            .enumerate()
            .map(|(i, (name, args))| {
                let entry = aura_tool_registry::agent_release::release()
                    .expect("Agent test setup and persisted assertion")
                    .entry(name)
                    .expect("Agent test setup and persisted assertion");
                aura_model_domain::NormalizedToolCall {
                    tool_call_id: ToolCallId::new(),
                    provider_call_id: format!("contending_provider_call_{i}")
                        .parse()
                        .expect("Agent test setup and persisted assertion"),
                    tool_name: aura_model_domain::provider_wire_tool_name(
                        name,
                        entry.definition.tool_id,
                    ),
                    position: i as u32,
                    arguments_json: steps::flat_arguments(args),
                }
            })
            .collect();
        let batch_id = ToolBatchId::new();
        self.service()
            .broker
            .lock()
            .map_err(|_| "the journal is unavailable".to_owned())?
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::agent::history::record_output(
                    tx,
                    &output,
                    Some(batch_id),
                    UtcTimestamp::now(),
                )?;
                let old = turns::load_run(tx, self.turn.agent_run_id)?;
                let mut next = old.clone();
                next.revision = Revision::new(old.revision.value() + 1);
                next.state = AgentRunState::ToolsRunning { tool_batch_id: batch_id };
                turns::save_run(tx, &old, &next)
            })
            .map_err(|error| format!("{error:?}"))?;
        self.service()
            .agent_execution_required()
            .admit_output_sync(self.step, self.settings.clone())
            .map_err(|error| format!("{error:?}"))
    }

    pub async fn execute(
        &self,
        batch: AdmittedBatch,
    ) -> aura_context_domain::HistoryAtomicGroupPreimageV1 {
        let (_sender, cancel) = tokio::sync::watch::channel(false);
        match self
            .service()
            .agent_execution_required()
            .execute_batch(batch, self.service().commands_required().clone(), cancel)
            .await
            .expect("Agent test setup and persisted assertion")
        {
            BatchOutcome::Completed(group) => group,
            BatchOutcome::AwaitingApproval { .. } => panic!("unexpected approval"),
            BatchOutcome::Unknown { attempts } => panic!("unexpected unknown: {attempts:?}"),
        }
    }

    /// Executes one batch and tolerates a **refusal** as a legitimate outcome.
    ///
    /// It exists for the regressions that assert a real protection still holds: a stale
    /// original-content condition *should* end as a refusal, so a helper that panics on a
    /// non-`Completed` outcome would forbid the very result under test. A transport-level error
    /// still panics, because that is a product failure rather than a decision.
    pub async fn execute_rejecting(&self, batch: AdmittedBatch) -> Option<BatchOutcome> {
        let (_sender, cancel) = tokio::sync::watch::channel(false);
        Some(
            self.service()
                .agent_execution_required()
                .execute_batch(batch, self.service().commands_required().clone(), cancel)
                .await
                .expect("the batch entry answered rather than failing"),
        )
    }
}
fn services(state: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(service) = &state.startup_mode else {
        panic!("Core unavailable")
    };
    service
}
