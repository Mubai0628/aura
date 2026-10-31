//! One-time recovery belongs to the candidate service before it becomes callable.
use crate::{RuntimeDependentRebuilder, agent_application, service_rebuild, spawn_p05_maintenance};
use aura_model_service::p05::P05Services;
use std::sync::Arc;
impl RuntimeDependentRebuilder {
    pub(crate) fn discharge_model_one_time_steps(
        &self,
        services: &P05Services,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::{ConstructionRefusal as Refusal, OneTimeStartupStep as Step};

        // The two reconciliation steps were performed by `P05Services::open`. When the startup path
        // never got that far, they are performed here through the same production entry, on a
        // clone of the supplied service, which is the process's one authority either way.
        let reconciliation = self.startup_ledger.claim_steps(&[
            Step::ModelConfigJournalReconciliation,
            Step::InterruptedModelOperationCancellation,
        ])?;
        if reconciliation.needs_work() {
            services.reconcile_at_live_supply().map_err(|error| {
                Refusal::Failed(format!(
                    "the model authority was supplied but its first recovery did not complete: \
                     {error}"
                ))
            })?;
            reconciliation.complete();
        }

        // The maintenance task follows the service it maintains. It is started here when the
        // startup path never started one, and retained by the process so it settles with the
        // runtime rather than leaking.
        let maintenance = self.startup_ledger.claim_steps(&[Step::ModelMaintenanceTask])?;
        if maintenance.needs_work() {
            // The maintenance task polls on its own timer, so it must be spawned with a reactor in
            // context. It is started on a thread that is not a runtime worker, with the process
            // handle entered, for the same reason the Agent recovery and backfill are: a recovery
            // can be invoked from an async caller, and spawning from a non-reactor thread panics.
            let Some(handle) = self.dependencies.runtime_handle.as_ref() else {
                return Err(Refusal::Failed(
                    "this process has no runtime handle, so the model maintenance task cannot start"
                        .to_owned(),
                ));
            };
            let handle = handle.clone();
            let settings = Arc::clone(&self.settings);
            let service = services.clone();
            let sync = self.sync.clone();
            let started = std::thread::scope(|scope| {
                scope
                    .spawn(move || {
                        let _entered = handle.enter();
                        spawn_p05_maintenance(&settings, service, sync)
                    })
                    .join()
            });
            let task = match started {
                Ok(Ok(task)) => task,
                Ok(Err(error)) => {
                    return Err(Refusal::Failed(format!(
                        "the model maintenance task could not be started: {error}"
                    )));
                }
                Err(_) => {
                    return Err(Refusal::Failed(
                        "the model maintenance task could not be started".to_owned(),
                    ));
                }
            };
            match self.extra_tasks.lock() {
                Ok(mut tasks) => tasks.push(task),
                Err(_) => {
                    task.0.abort();
                    return Err(Refusal::Failed("maintenance ownership is unavailable".into()));
                }
            }
            maintenance.complete();
        }
        Ok(())
    }
    pub(crate) fn discharge_agent_one_time_steps(
        &self,
        agent: &Arc<agent_application::AgentApplication>,
    ) -> std::result::Result<(), service_rebuild::ConstructionRefusal> {
        use service_rebuild::{ConstructionRefusal as Refusal, OneTimeStartupStep as Step};

        let reconciliation = self
            .startup_ledger
            .claim_steps(&[Step::AgentInboxStartupScan, Step::AgentWaitRestoration])?;
        if reconciliation.needs_work() {
            // The scan and the wait restoration are both async; they are driven on the process's
            // own runtime handle, which is the runtime that owns every other long-lived Core task.
            let Some(handle) = self.dependencies.runtime_handle.as_ref() else {
                return Err(Refusal::Failed(
                    "this process has no runtime handle, so the Agent first recovery cannot run"
                        .to_owned(),
                ));
            };
            agent.reconcile_at_live_supply(handle).map_err(|error| {
                Refusal::Failed(format!(
                    "the Agent application was supplied but its first recovery did not complete: \
                     {error}"
                ))
            })?;
            reconciliation.complete();
        }
        let backfill = self.startup_ledger.claim_steps(&[Step::AgentDerivedBackfill])?;
        if backfill.needs_work() {
            // The backfill owner spawns a task, so it needs a reactor. It is started on a thread
            // that is not a runtime worker, with the process handle entered, for the same reason the
            // recovery scan is: a recovery can be invoked from an async caller, and spawning from a
            // non-reactor thread panics.
            let Some(handle) = self.dependencies.runtime_handle.as_ref() else {
                return Err(Refusal::Failed(
                    "this process has no runtime handle, so the derived backfill cannot start"
                        .to_owned(),
                ));
            };
            let handle = handle.clone();
            let agent = Arc::clone(agent);
            let started = std::thread::scope(|scope| {
                scope
                    .spawn(move || {
                        let _entered = handle.enter();
                        agent.start_derived_backfill();
                    })
                    .join()
            });
            if started.is_err() {
                return Err(Refusal::Failed(
                    "the derived backfill owner could not be started".to_owned(),
                ));
            }
            backfill.complete();
        }
        Ok(())
    }
}
