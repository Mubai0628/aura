//! The worker owns the Job until accounting proves it empty; Core owns receipts.
use super::{CompatBackend, soft_proxy::SoftProxy};
use crate::controller::{
    LaunchLease, NotStarted, PortSection, WindowsUserCorePort, WindowsUserCorePortSection,
};
use anyhow::Result;
use aura_domain_common::{UtcTimestamp, jcs::canonical_sha256};
use aura_execution_contract::{DispatchRequest, OperationNetwork, TerminationEvidence};
use aura_windows_user_contract::{
    ManagedShellCommand, RunnerObservation, RunnerStopReason, WindowsUserPreparation,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::watch;

pub(super) struct Record {
    pub request: DispatchRequest,
    pub port: Arc<dyn WindowsUserCorePort>,
    pub reactor: tokio::runtime::Handle,
}
impl Record {
    pub fn write<T: Send + 'static>(
        &self,
        name: &'static str,
        work: impl FnOnce(Arc<dyn WindowsUserCorePort>, DispatchRequest) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let port = self.port.clone();
        let request = self.request.clone();
        self.reactor.block_on(self.port.section(
            PortSection::write(name, request.operation.content.identity.agent_run_id),
            Box::new(move || work(port, request)),
        ))
    }
}

pub(super) async fn run(
    request: DispatchRequest,
    prepared: WindowsUserPreparation,
    engine: Arc<CompatBackend>,
    proxy: Arc<SoftProxy>,
    expires: UtcTimestamp,
    cancelled: watch::Receiver<bool>,
    port: Arc<dyn WindowsUserCorePort>,
) -> Result<TerminationEvidence> {
    if *cancelled.borrow() || UtcTimestamp::now() >= expires.min(prepared.valid_until) {
        let reason = if *cancelled.borrow() { NotStarted::Cancelled } else { NotStarted::Expired };
        return not_started(port, request, reason).await;
    }
    let launch = match port.begin_launch(&request).await {
        Ok(lease) => lease,
        Err(_) => return not_started(port, request, NotStarted::Rejected).await,
    };
    let record = Record {
        request: request.clone(),
        port: port.clone(),
        reactor: tokio::runtime::Handle::current(),
    };
    let observation = crate::observation::ExecutionObservation::capture(port.as_ref(), &request);
    let worked = tokio::task::spawn_blocking(move || {
        observe(record, prepared, engine, proxy, cancelled, launch, observation)
    })
    .await;
    match worked {
        Ok(Ok(evidence)) => Ok(evidence),
        // Losing an observation does not prove that no process ran.
        _ => {
            let writer = port.clone();
            port.section(
                PortSection::write(
                    "compat.unknown",
                    request.operation.content.identity.agent_run_id,
                ),
                Box::new(move || {
                    writer.complete(
                        &request,
                        None,
                        false,
                        false,
                        None,
                        Some("compat.observation_unknown"),
                    )
                }),
            )
            .await
        }
    }
}

async fn not_started(
    port: Arc<dyn WindowsUserCorePort>,
    request: DispatchRequest,
    reason: NotStarted,
) -> Result<TerminationEvidence> {
    let writer = port.clone();
    port.section(
        PortSection::write("compat.not_started", request.operation.content.identity.agent_run_id),
        Box::new(move || writer.complete(&request, None, true, true, Some(reason), None)),
    )
    .await
}

#[allow(clippy::too_many_arguments)] // The real worker owns its start lease and observation.
fn observe(
    record: Record,
    prepared: WindowsUserPreparation,
    engine: Arc<CompatBackend>,
    proxy: Arc<SoftProxy>,
    cancelled: watch::Receiver<bool>,
    launch: LaunchLease,
    mut observation: crate::observation::ExecutionObservation,
) -> Result<TerminationEvidence> {
    if *cancelled.borrow() {
        return record.write("compat.cancel_before_launch", |port, request| {
            port.complete(&request, None, true, true, Some(NotStarted::Cancelled), None)
        });
    }
    let command: ManagedShellCommand =
        serde_json::from_value(record.request.operation.content.action.clone())?;
    let started_at = UtcTimestamp::now();
    let offline = record.request.operation.content.network == OperationNetwork::None;
    let mut execution = match engine.launch(&command, offline.then_some(proxy.endpoint.as_str())) {
        Ok(execution) => execution,
        Err(error) => {
            let no_start = error
                .downcast_ref::<aura_windows_platform::JobLaunchError>()
                .is_some_and(|error| !error.timing.root_created || error.timing.failure_stopped);
            return record.write("compat.launch_failed", move |port, request| {
                port.complete(
                    &request,
                    None,
                    no_start,
                    no_start,
                    no_start.then_some(NotStarted::Rejected),
                    Some("compat.launch_failed"),
                )
            });
        }
    };
    let process_id = execution.root_identity().process_id;
    observation.started();
    let started = record.write("compat.started", move |port, request| {
        port.started(&request, process_id, started_at)
    });
    drop(launch);
    if started.is_err() {
        let stopped = execution.stop(Duration::from_secs(3)).confirmed;
        drop(execution);
        return record.write("compat.start_record_failed", move |port, request| {
            port.complete(
                &request,
                None,
                stopped,
                stopped,
                None,
                Some("compat.start_record_failed"),
            )
        });
    }
    let names = execution.excluded_environment_names().to_vec();
    record.write("compat.environment", move |port, request| {
        port.filtered_environment_names(&request, &names)
    })?;
    let direct = match execution.run_direct_until(|| *cancelled.borrow()) {
        Ok(direct) => direct,
        Err(_) => {
            let stopped = execution.stop(Duration::from_secs(3)).confirmed;
            drop(execution);
            return record.write("compat.observe_failed", move |port, request| {
                port.complete(
                    &request,
                    None,
                    stopped,
                    stopped,
                    None,
                    Some("compat.observation_failed"),
                )
            });
        }
    };
    let value = RunnerObservation {
        attempt: record.request.binding.attempt.clone(),
        binding_fingerprint: canonical_sha256(&record.request.binding)?,
        account_sid: prepared.account_sid,
        process_id,
        shell_image: prepared.shell_image,
        started_at,
        finished_at: UtcTimestamp::now(),
        exit_code: direct.exit_code,
        stop_reason: match direct.stop_reason {
            "exited" => RunnerStopReason::Exited,
            "cancelled" => RunnerStopReason::Cancelled,
            "timed_out" => RunnerStopReason::TimedOut,
            "output_limit" => RunnerStopReason::OutputLimit,
            _ => RunnerStopReason::Unknown,
        },
        process_tree_stopped: direct.stop_confirmed,
        stdout: direct.stdout.clone(),
        stderr: direct.stderr.clone(),
        submitted_script: if command.shell == aura_windows_user_contract::ManagedShell::Cmd {
            std::fs::read_to_string(execution.scratch_directory().join("command.cmd"))?
        } else {
            crate::environment::submitted_powershell(
                &std::fs::read_to_string(execution.scratch_directory().join("command.ps1"))?,
                &std::fs::read_to_string(
                    execution.scratch_directory().join(crate::environment::POWERSHELL_SCRIPT_FILE),
                )?,
            )
        },
        output_truncated: direct.output_truncated,
    };
    observation.finish();
    let stopped = direct.stop_confirmed;
    if stopped {
        drop(execution);
        return publish_direct(&record, value, true);
    }
    let _ = publish_direct(&record, value, false)?;
    super::background::follow(
        &record,
        &mut execution,
        direct.stdout.len(),
        direct.stderr.len(),
        &cancelled,
    )?;
    let stopped = execution.active_processes().is_ok_and(|active| active == 0);
    drop(execution);
    record.write("compat.runtime_stopped", move |port, request| {
        port.runtime_stopped(&request, stopped, stopped)
    })
}

fn publish_direct(
    record: &Record,
    value: RunnerObservation,
    stopped: bool,
) -> Result<TerminationEvidence> {
    record.write("compat.complete", move |port, request| {
        port.complete(&request, Some(&value), stopped, true, None, None)
    })
}
