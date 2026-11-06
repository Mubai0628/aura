//! Before admission, what the step's computer calls need is read from the desktop: the programs
//! they name and, for `computer.apps`, which of them have a window. And whether the computer
//! tools are offered to a run's model at all.
use super::*;
use aura_agent_execution::agent_execution;

impl AgentApplication {
    pub(super) async fn prepare_computer_calls(
        &self,
        run_id: AgentRunId,
        step_id: ModelStepId,
    ) -> anyhow::Result<()> {
        let on = self.execution.desktop.get().is_some_and(|desktop| {
            let settings = desktop.settings();
            settings.settings.enabled && settings.issue.is_none()
        });
        if !on {
            return Ok(());
        }
        let (calls, mode) = self
            .store
            .read(StoreStep::run(run_id, "agent.computer.calls"), move |db| {
                Ok((
                    aura_persistence_v2::hooks::calls_before_admission(db, step_id)?,
                    store::configuration(db, run_id)?.request.computer_mode.unwrap_or_default(),
                ))
            })
            .await?;
        let calls = calls
            .into_iter()
            .filter(|call| call.tool_name.starts_with("computer."))
            .collect::<Vec<_>>();
        if calls.is_empty() {
            return Ok(());
        }
        let session = self.turn(run_id).await?.session_id;
        agent_execution::prepare_computer_calls(&self.execution, session, mode, calls).await;
        Ok(())
    }

    /// Whether the computer tools are offered to a run of this model: computer control is on,
    /// and the model is one the settings name or, when they name none, one that takes images.
    pub(super) fn computer_offered(&self, profile: ModelProfileId) -> bool {
        let Some(desktop) = self.execution.desktop.get() else { return false };
        let settings = desktop.settings();
        if !settings.settings.enabled || settings.issue.is_some() {
            return false;
        }
        if !settings.settings.models.is_empty() {
            return settings.settings.models.contains(&profile.to_string());
        }
        self.p05.model_config.snapshot().is_ok_and(|config| {
            config.registry.models.get(&profile).is_some_and(|model| {
                aura_model_domain::takes_images(&model.request_defaults, model.catalog.as_ref())
            })
        })
    }
}
