use crate::{
    LaunchStage, LaunchStageKind,
    controller::{
        NotStarted, PortSection, WindowsUserCorePort, WindowsUserCorePortSection, budgets,
    },
};
use anyhow::{Result, ensure};
use aura_domain_common::UtcTimestamp;
use aura_execution_contract::{DispatchRequest, TerminationEvidence};
use aura_windows_user_contract::*;
use std::{path::Path, sync::Arc, time::Duration};
use tokio::sync::{Semaphore, watch};

mod boundary;

pub(crate) use boundary::{LaunchBoundary, LaunchFacts, RunnerLaunch, SystemBoundary};

/// Reports one measured sub-stage.
///
/// Telemetry never decides whether a launch proceeds: a journal that cannot record
/// a measurement loses the measurement, and the launch continues. Only stages that
/// were actually reached are reported, so an absent line means "not measured here",
/// not "0 ms".
///
/// Boundary: the write is a journal transaction, so it goes to the port's bounded
/// owner; the caller awaits its answer and still ignores it exactly as before.
async fn stage(
    port: &Arc<dyn WindowsUserCorePort>,
    request: &DispatchRequest,
    kind: LaunchStageKind,
    since: std::time::Instant,
) -> u64 {
    let ms = elapsed_ms(since);
    let recorder = Arc::clone(port);
    let owned = request.clone();
    let _ = port
        .section(
            PortSection::write(
                "runner.launch_stage",
                request.operation.content.identity.agent_run_id,
            ),
            Box::new(move || recorder.launch_stage(&owned, LaunchStage { stage: kind, ms })),
        )
        .await;
    ms
}

/// Reports one launch's bounded workload counts exactly once, on the way out of
/// the launch — including the paths that leave early because the gate wait expired,
/// the plan was already stale, the launch was cancelled, the handshake failed or
/// the Runner rejected it.
///
/// The owner sits *before* the gate wait so no exit can bypass the record: a
/// cancelled or expired launch still did its queue wait, and that wait is this
/// launch's work to report. The write's failure is ignored on purpose: a journal
/// that cannot hold a measurement loses the measurement, and never fails a launch
/// the user asked for. Counts start at zero for every launch and are never carried
/// over from the previous call.
struct WorkReport {
    port: Arc<dyn WindowsUserCorePort>,
    request: DispatchRequest,
    work: crate::LaunchWorkV1,
    /// Whether this report already reached the port. Every normal exit awaits
    /// [`WorkReport::flush`]; only an unwind leaves it false for `Drop`.
    flushed: bool,
}

impl WorkReport {
    fn work(&mut self) -> &mut crate::LaunchWorkV1 {
        &mut self.work
    }

    /// Records this launch's workload counts once, on the port's bounded owner.
    ///
    /// The record has to land before the terminal record on every exit path, so every exit
    /// path awaits this instead of relying on `Drop`.
    async fn flush(&mut self) {
        if self.flushed {
            return;
        }
        // Marked before the write, so the unwind fallback in `Drop` can never be a second writer
        // of a record this call already made.
        self.flushed = true;
        let port = Arc::clone(&self.port);
        let recorder = Arc::clone(&self.port);
        let request = self.request.clone();
        let work = self.work;
        let _ = port
            .section(
                PortSection::write(
                    "runner.launch_work",
                    request.operation.content.identity.agent_run_id,
                ),
                Box::new(move || recorder.launch_work(&request, work)),
            )
            .await;
    }
}

impl Drop for WorkReport {
    fn drop(&mut self) {
        // Reached only while the launch is unwinding: every normal exit awaits `flush`, which sets
        // the flag. An unwind cannot await, so this last-resort write is the one synchronous form
        // that remains, and it is named as such here instead of being presented as bounded.
        if self.flushed {
            return;
        }
        self.flushed = true;
        let _ = self.port.launch_work(&self.request, self.work);
    }
}

/// How often a pending launch claim re-reads the stop signal and the launch
/// window while the authority answers. The wait is a `select!` over the claim,
/// the stop signal and this tick; the tick only bounds how late a cancellation
/// or an expired window is noticed when the claim future stays pending, and
/// never extends the window.
const CLAIM_WAIT_TICK: Duration = Duration::from_millis(100);

#[allow(clippy::too_many_arguments)] // Dispatch, preparation, installation and Core port are distinct authorities.
pub(crate) async fn run(
    request: DispatchRequest,
    prepared: WindowsUserPreparation,
    facts: LaunchFacts,
    expiry: UtcTimestamp,
    mut cancelled: watch::Receiver<bool>,
    gate: Arc<Semaphore>,
    port: Arc<dyn WindowsUserCorePort>,
    boundary: Arc<dyn LaunchBoundary>,
    mut execution_observation: crate::observation::ExecutionObservation,
) -> Result<TerminationEvidence> {
    let mut report = WorkReport {
        port: Arc::clone(&port),
        request: request.clone(),
        work: crate::LaunchWorkV1::default(),
        flushed: false,
    };
    // Budgets are read once, at the start, and are separate facts from then on. The
    // queue wait, the resource preparation, the launch window and the script have
    // nothing to do with each other, and a launch that spends one must not be refused
    // for having spent another.
    let queue_started = std::time::Instant::now();
    let permit = tokio::select! {
        value = tokio::time::timeout(budgets::QUEUE_BUDGET, gate.acquire_owned()) => value.ok().and_then(std::result::Result::ok),
        _ = cancelled.wait_for(|v| *v) => None,
    };
    report.work().queue_ms = elapsed_ms(queue_started);
    if permit.is_none() || *cancelled.borrow() {
        let reason = if *cancelled.borrow() { NotStarted::Cancelled } else { NotStarted::Expired };
        report.flush().await;
        let recorder = Arc::clone(&port);
        let owned = request.clone();
        return port
            .section(
                PortSection::write(
                    "runner.complete",
                    request.operation.content.identity.agent_run_id,
                ),
                Box::new(move || recorder.complete(&owned, None, true, true, Some(reason), None)),
            )
            .await;
    }
    // The plan bound is checked once, before any resource work: a preparation whose
    // captured facts are too old must not start spending access-control work.
    if boundary.now() >= prepared.valid_until {
        report.flush().await;
        let recorder = Arc::clone(&port);
        let owned = request.clone();
        return port
            .section(
                PortSection::write(
                    "runner.complete",
                    request.operation.content.identity.agent_run_id,
                ),
                Box::new(move || {
                    recorder.complete(&owned, None, true, true, Some(NotStarted::Expired), None)
                }),
            )
            .await;
    }
    let mut permit = permit;
    let mut job = None;
    let mut nonce = None;
    let mut direct_published = false;
    let mut launch_rejected = false;
    let mut execute_sent = false;
    // The Runner's own bounded rejection reason, recorded once where it was
    // stated. The final receipt carries it as the failure code instead of the
    // generic "runner.execution_rejected" wrapper it bails with.
    let mut rejected_reason: Option<String> = None;
    let mut launch_lease = None;
    let mut eligibility: Option<(UtcTimestamp, u64)> = None;
    let attempt = request.binding.attempt.clone();
    let observation = async {
        let mut mark = std::time::Instant::now();
        let online = prepared.mode == aura_security_contract::PermissionMode::FullAccess;
        let password = boundary.verify_installation(&facts, online, report.work())?;
        let mut random = [0_u8; 16];
        getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("runner.random_unavailable"))?;
        let suffix = random.iter().map(|v| format!("{v:02x}")).collect::<String>();
        nonce = Some(suffix.clone());
        let write_sid = format!("S-1-5-21-{}", random.chunks_exact(4).map(|v| u32::from_le_bytes(v.try_into().expect("four bytes")).to_string()).collect::<Vec<_>>().join("-"));
        let scratch_path = facts.root.join("execution-scratch").join(&suffix);
        stage(&port, &request, LaunchStageKind::Verified, mark).await;
        mark = std::time::Instant::now();
        let mut scratch = None;
        report.work().runtime_reused = boundary.runtime_reused();
        // The budget for touching resources starts here, not before the queue wait: the
        // wait is its own budget and must not eat this one. The cancel signal is the
        // launch's own live watch state, polled between objects, so a stop raised while
        // the walk is running ends the walk instead of being discovered after it.
        let preparation_started = std::time::Instant::now();
        let preparation_deadline = preparation_started + budgets::PREPARATION_BUDGET;
        let live_cancel = cancelled.clone();
        let grant_control = crate::file_lease::GrantControl::new(
            preparation_deadline,
            crate::file_lease::GrantSignal::new(move || *live_cancel.borrow()),
        );
        let allocated = boundary.allocate_resources(
            &facts,
            &suffix,
            &facts.account.sid,
            &prepared.read_roots,
            &mut |files, registered_scratch| {
                let (_, _parent) =
                    crate::file_binding::capture(&facts.root.join("execution-scratch"), false)?;
                std::fs::create_dir(&scratch_path)?;
                let (binding, _) = crate::file_binding::capture(&scratch_path, false)?;
                *registered_scratch = Some(binding.clone());
                // Work is recorded before the next step can fail, so a pass that stops
                // partway still reports what it did instead of losing it. A stopped
                // pass is never a finished grant: finish() refuses it and the launch
                // ends before any process exists.
                let scratch_outcome =
                    files.grant_tree(&binding, &facts.account.sid, Some(&write_sid), &grant_control);
                report.work().record_grant_attempt(&scratch_outcome);
                scratch_outcome.finish()?;
                // Keep the potentially large workspace tree walk before Runner
                // startup; Ready has a bounded wait for Execute. Only the small
                // ancestor metadata check needs the authenticated Runner token.
                for root in &prepared.read_roots {
                    let grant = files.grant_tree(
                        root,
                        &facts.account.sid,
                        prepared.write_roots.contains(root).then_some(write_sid.as_str()),
                        &grant_control,
                    );
                    report.work().record_grant_attempt(&grant);
                    grant.finish()?;
                }
                scratch = Some(binding.clone());
                Ok(())
            },
        );
        // The recovery block's cost belongs to the launch that opened it; on a
        // reused runtime this returns None and the launch reports no recovery.
        if let Some(recovery) = boundary.take_recovery_work() {
            report.work().record_recovery(recovery);
        }
        // The preparation span is measured no matter how the pass ended: a launch that
        // spent forty seconds and was then stopped still spent forty seconds, and the
        // record has to say so instead of reporting the zero of a span nobody closed.
        report.work().preparation_ms = elapsed_ms(preparation_started);
        let resources = boundary.resource_counts();
        report.work().active_resources = resources.active;
        report.work().background_resources = resources.background;
        job = Some(allocated?);
        stage(&port, &request, LaunchStageKind::ResourcesAllocated, mark).await;
        mark = std::time::Instant::now();
        let startup_started = mark;
        let scratch = scratch.ok_or_else(|| anyhow::anyhow!("runner.scratch_missing"))?;
        ensure!(!*cancelled.borrow(), "runner.cancelled_before_launch");
        // The launch window is decided here, *after* the resources exist and *after*
        // Core has re-read the live authority below. Preparation can therefore never
        // spend it: the window is as fresh at dispatch as it is at this line. It is
        // still bounded by the authority's own deadline and by the plan bound, so a
        // short ticket can never outlive either.
        let issued_at = boundary.now();
        let launch_until = launch_window(issued_at, expiry, prepared.valid_until)
            .ok_or_else(|| anyhow::anyhow!("runner.launch_eligibility_expired"))?;
        let launch_window_ms =
            u64::try_from((launch_until.value() - issued_at.value()).whole_milliseconds())
                .unwrap_or(0);
        eligibility = Some((launch_until, launch_window_ms));
        report.work().launch_window_ms = launch_window_ms;
        let pipe_name = format!(r"\\.\pipe\Aura.Execution.{suffix}");
        let environment =
            crate::environment::baseline(&scratch_path, Some(&facts.root.join("runtime")))?;
        let runner_job = job.as_ref().ok_or_else(|| anyhow::anyhow!("runner.job_missing"))?;
        let mut transport = boundary.launch_runner(&RunnerLaunch {
            job: runner_job,
            pipe_name: &pipe_name,
            account_name: &facts.account.name,
            account_sid: &facts.account.sid,
            password: &password,
            executable: Path::new(&facts.runner_image.path),
            arguments: &[
                "--core".into(),
                facts.root.to_string_lossy().into_owned(),
                std::process::id().to_string(),
                pipe_name.clone(),
            ],
            directory: &facts.root,
            environment: &environment,
        })?;
        // Pipe and process are created inside the one platform spawn; both stages were
        // reached, measured from the same mark.
        stage(&port, &request, LaunchStageKind::PipeCreated, mark).await;
        mark = std::time::Instant::now();
        stage(&port, &request, LaunchStageKind::ProcessCreated, mark).await;
        mark = std::time::Instant::now();
        let identity = transport.identity()?;
        let (actual, _image) = crate::file_binding::capture(&identity.executable_path, true)?;
        ensure!(actual == facts.runner_image
            && aura_windows_platform::process_account_sid(&identity)? == facts.account.sid,
            "runner.launched_identity_mismatch");
        tokio::select! {
            connected = tokio::time::timeout(budgets::STARTUP_HANDSHAKE, transport.connect()) => connected??,
            _ = cancelled.wait_for(|v| *v) => anyhow::bail!("runner.cancelled_before_connect"),
        }
        stage(&port, &request, LaunchStageKind::PipeConnected, mark).await;
        mark = std::time::Instant::now();
        transport.authenticate_client()?;
        ensure!(!*cancelled.borrow(), "runner.cancelled_before_execute");
        // Wait for identity verification. Execute must not be queued while the
        // Runner is still opening the installation or about to reject it.
        let ready = tokio::time::timeout(budgets::STARTUP_HANDSHAKE, transport.read_ready()).await;
        match ready?? {
            RunnerCoreMessage::Ready => {}
            RunnerCoreMessage::Rejected { reason } => {
                launch_rejected = true;
                ensure!(reason.len() <= 128 && reason.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)), "runner.invalid_diagnostic");
                rejected_reason = Some(reason.clone());
                {
                    // Boundary: the rejection diagnostic, one bounded write.
                    let recorder = Arc::clone(&port);
                    let owned = request.clone();
                    let reason = reason.clone();
                    port.section(
                        PortSection::write(
                            "runner.diagnostic",
                            owned.operation.content.identity.agent_run_id,
                        ),
                        Box::new(move || recorder.diagnostic(&owned, &reason)),
                    )
                    .await?;
                }
                anyhow::bail!("runner.execution_rejected");
            }
            _ => anyhow::bail!("runner.ready_required"),
        }
        stage(&port, &request, LaunchStageKind::RunnerReady, mark).await;
        mark = std::time::Instant::now();
        {
            // The metadata access wraps an Authz context and is not Send; it is
            // created, used and dropped inside this synchronous block, before the
            // next await point.
            let access = boundary.runner_metadata_access(&identity, &facts.account.sid)?;
            boundary.prepare_workspace(&suffix, &mut |files| {
                for root in &prepared.read_roots {
                    let outcome = files.grant_ancestor_metadata(
                        root,
                        &access,
                        &prepared.read_roots,
                        &grant_control,
                    );
                    let work = outcome.work;
                    report.work().record_ancestors(work);
                    outcome.finish()?;
                }
                Ok(())
            })?;
        }
        stage(&port, &request, LaunchStageKind::WorkspaceGranted, mark).await;
        mark = std::time::Instant::now();
        let launch_until = eligibility.expect("the launch window was decided above").0;
        ensure!(
            !*cancelled.borrow() && boundary.now() < launch_until,
            "runner.cancelled_before_execute"
        );
        let envelope = RunnerDispatchEnvelope { schema_version: Default::default(), dispatch: request.clone(),
            expires_at: launch_until, attempt_write_sid: write_sid, scratch_directory: scratch };
        envelope.validate(boundary.now())?;
        // `begin_launch` is where the authority is re-read: live mode and revision,
        // the exact scope, the checkpoint, the recorded dispatch authority and the
        // claimed Attempt. A launch that preparation took a long time over is still
        // checked against the authority as it is *now*, which is what makes a fresh
        // short window safe rather than merely convenient.
        let mut claim = port.begin_launch(&request);
        loop {
            // The claim wait races the launch's own stop signal and its own
            // window; it never extends either. Abandoning the wait drops the
            // in-flight future, whose guards are the only handle to the claim:
            // no lease value reaches this launch, `launch_lease` stays empty,
            // and the runner process, the job tree and the granted resources
            // unwind through the same pre-Execute cleanup as every other
            // NotStarted exit.
            ensure!(!*cancelled.borrow(), "runner.cancelled_before_execute");
            ensure!(boundary.now() < launch_until, "runner.launch_window_expired");
            tokio::select! {
                lease = &mut claim => {
                    launch_lease = Some(lease?);
                    break;
                }
                _ = cancelled.wait_for(|v| *v) => anyhow::bail!("runner.cancelled_before_execute"),
                _ = tokio::time::sleep(CLAIM_WAIT_TICK) => {}
            }
        }
        stage(&port, &request, LaunchStageKind::LaunchClaimed, mark).await;
        mark = std::time::Instant::now();
        // The claim returned; the stop signal and the launch window are re-read
        // before the first Execute byte. None is written yet here, so either
        // exit stays a clean NotStarted instead of the partial-frame Unknown below.
        ensure!(
            !*cancelled.borrow() && boundary.now() < launch_until,
            "runner.cancelled_before_execute"
        );
        let (mut reader, mut writer) = transport.split_for_execute();
        // Even a partial frame may have dispatched. Never report NoEffect
        // merely because cancellation races a broken response connection.
        execute_sent = true;
        writer.write(&CoreRunnerMessage::Execute { envelope: Box::new(envelope) }).await?;
        report.work().startup_ms = elapsed_ms(startup_started);
        stage(&port, &request, LaunchStageKind::Dispatched, mark).await;
        let command: ManagedShellCommand = serde_json::from_value(request.operation.content.action.clone())?;
        let deadline = tokio::time::sleep(Duration::from_millis(command.timeout_ms).saturating_add(Duration::from_secs(15)));
        tokio::pin!(deadline);
        let mut cancel_sent = false;
        let mut process_id = None;
        let mut direct_observation = None;
        let mut background_sequence = 0;
        let mut retained_bytes = 0usize;
        let mut progress_sequence = 0;
        let mut progress_bytes = 0usize;
        // Keep the same read future across cancellation; a partial frame is
        // never discarded and reinterpreted as a new length prefix.
        loop {
            let receive = reader.read();
            tokio::pin!(receive);
            let message = loop {
                tokio::select! {
                    message = &mut receive => break message?,
                    _ = async { let _ = cancelled.wait_for(|v| *v).await; }, if !cancel_sent => {
                        writer.write(&CoreRunnerMessage::Cancel { attempt: attempt.clone() }).await?;
                        cancel_sent = true;
                    }
                    _ = &mut deadline, if !direct_published => anyhow::bail!("runner.execution_response_timeout"),
                }
            };
            match message {
                RunnerCoreMessage::Progress { update } => {
                    ensure!(!direct_published && update.attempt == attempt && update.sequence == progress_sequence + 1
                        && (update.submitted_script.is_none() || update.sequence == 1), "runner.progress_binding");
                    progress_bytes = progress_bytes.saturating_add(update.stdout.len()).saturating_add(update.stderr.len());
                    ensure!(progress_bytes <= command.max_output_bytes as usize, "runner.progress_output_limit");
                    progress_sequence = update.sequence;
                    {
                        // Boundary: the progress record, one bounded write.
                        let recorder = Arc::clone(&port);
                        let owned = request.clone();
                        port.section(
                            PortSection::write(
                                "runner.progress",
                                owned.operation.content.identity.agent_run_id,
                            ),
                            Box::new(move || recorder.progress(&owned, &update)),
                        )
                        .await?;
                    }
                }
                RunnerCoreMessage::Started { attempt: started_attempt, process_id: pid } => {
                    ensure!(started_attempt == attempt && process_id.is_none() && pid != 0, "runner.started_binding");
                    process_id = Some(pid);
                    execution_observation.started();
                    {
                        // Boundary: the start record, one bounded write.
                        let recorder = Arc::clone(&port);
                        let owned = request.clone();
                        let at = UtcTimestamp::now();
                        port.section(
                            PortSection::write(
                                "runner.started",
                                owned.operation.content.identity.agent_run_id,
                            ),
                            Box::new(move || recorder.started(&owned, pid, at)),
                        )
                        .await?;
                    }
                    launch_lease.take();
                }
                RunnerCoreMessage::Completed { observation } => {
                    ensure!(observation.attempt == attempt && observation.binding_fingerprint == aura_domain_common::jcs::canonical_sha256(&request.binding)?
                        && observation.account_sid == prepared.account_sid && observation.shell_image == prepared.shell_image
                        && process_id == Some(observation.process_id) && !direct_published
                        && observation.stdout.len().saturating_add(observation.stderr.len()) <= command.max_output_bytes as usize
                        && observation.finished_at >= observation.started_at, "runner.result_binding");
                    // Verified foreground completion is independent of children and journaling.
                    execution_observation.finish();
                    if observation.process_tree_stopped { break Ok::<_, anyhow::Error>(*observation); }
                    retained_bytes = observation.stdout.len() + observation.stderr.len();
                    {
                        // Boundary: the direct terminal record, one bounded write.
                        let recorder = Arc::clone(&port);
                        let owned = request.clone();
                        let direct = (*observation).clone();
                        port.section(
                            PortSection::write(
                                "runner.complete",
                                owned.operation.content.identity.agent_run_id,
                            ),
                            Box::new(move || {
                                recorder.complete(&owned, Some(&direct), false, false, None, None)
                            }),
                        )
                        .await?;
                    }
                    direct_published = true;
                    permit.take();
                    direct_observation = Some(*observation);
                }
                RunnerCoreMessage::Background { update } => {
                    ensure!(direct_published && update.attempt == attempt && update.sequence == background_sequence + 1, "runner.background_binding");
                    retained_bytes = retained_bytes.saturating_add(update.stdout.len()).saturating_add(update.stderr.len());
                    ensure!(retained_bytes <= command.max_output_bytes as usize, "runner.background_output_limit");
                    background_sequence = update.sequence;
                    let finished = update.state != BackgroundState::Running;
                    {
                        // Boundary: the background record, one bounded write.
                        let recorder = Arc::clone(&port);
                        let owned = request.clone();
                        port.section(
                            PortSection::write(
                                "runner.background",
                                owned.operation.content.identity.agent_run_id,
                            ),
                            Box::new(move || recorder.background(&owned, &update)),
                        )
                        .await?;
                    }
                    if finished {
                        break Ok(direct_observation.expect("direct observation"));
                    }
                }
                RunnerCoreMessage::Rejected { reason } => {
                    launch_rejected = process_id.is_none();
                    ensure!(reason.len() <= 128 && reason.bytes().all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b)), "runner.invalid_diagnostic");
                    rejected_reason = Some(reason.clone());
                    {
                        // Boundary: the rejection diagnostic, one bounded write.
                        let recorder = Arc::clone(&port);
                        let owned = request.clone();
                        let stated = reason.clone();
                        port.section(
                            PortSection::write(
                                "runner.diagnostic",
                                owned.operation.content.identity.agent_run_id,
                            ),
                            Box::new(move || recorder.diagnostic(&owned, &stated)),
                        )
                        .await?;
                    }
                    anyhow::bail!("runner.execution_rejected");
                }
                RunnerCoreMessage::Ready => anyhow::bail!("runner.ready_after_execute"),
            }
        }
    }.await;
    // Core confirms the outer Job is empty even when Runner claims termination.
    let mut tree_stopped = job.is_none();
    if let Some(job) = job.as_ref() {
        let _ = job.terminate_tree();
        for _ in 0..100 {
            if job.active_processes().ok() == Some(0) {
                tree_stopped = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
    drop(job);
    drop(launch_lease);
    // The dispatch error's bounded code is a receipt fact: without it the model
    // and the audit trail only see an unexplained rejection. A Runner rejection
    // keeps its own stated reason, which was already diagnosed where it arrived;
    // every other code still needs its own record here. The restore failure
    // below is reported on its own, never folded into this code.
    let failure_code = observation.as_ref().err().and_then(|error| {
        rejected_reason.clone().or_else(|| crate::controller::bounded_failure_code(error))
    });
    if rejected_reason.is_none()
        && let Some(code) = &failure_code
    {
        // Boundary: the dispatch code's own record, one bounded write.
        let recorder = Arc::clone(&port);
        let owned = request.clone();
        let stated = code.clone();
        let _ = port
            .section(
                PortSection::write(
                    "runner.diagnostic",
                    owned.operation.content.identity.agent_run_id,
                ),
                Box::new(move || recorder.diagnostic(&owned, &stated)),
            )
            .await;
    }
    let cleanup = if tree_stopped {
        match nonce.as_ref().map_or(Ok(true), |nonce| boundary.stop_resources(nonce)) {
            Ok(done) => done,
            Err(error) => {
                let code = crate::controller::bounded_failure_code(&error)
                    .unwrap_or_else(|| "runner.resource_restore_failed".to_owned());
                let recorder = Arc::clone(&port);
                let owned = request.clone();
                let _ = port
                    .section(
                        PortSection::write(
                            "runner.diagnostic",
                            owned.operation.content.identity.agent_run_id,
                        ),
                        Box::new(move || recorder.diagnostic(&owned, &code)),
                    )
                    .await;
                false
            }
        }
    } else {
        false
    };
    // The workload record lands before the terminal record on every exit path.
    report.flush().await;
    if direct_published {
        // The direct Tool result is immutable. Late transport loss/cancellation
        // is appended to its runtime record and never fabricates a second call.
        let recorder = Arc::clone(&port);
        let owned = request.clone();
        return port
            .section(
                PortSection::write(
                    "runner.runtime_stopped",
                    owned.operation.content.identity.agent_run_id,
                ),
                Box::new(move || recorder.runtime_stopped(&owned, tree_stopped, cleanup)),
            )
            .await;
    }
    let launch_until =
        eligibility.map(|(until, _)| until).unwrap_or(expiry.min(prepared.valid_until));
    let not_started = ((!execute_sent || launch_rejected) && observation.is_err() && tree_stopped)
        .then(|| {
            if *cancelled.borrow() {
                NotStarted::Cancelled
            } else if boundary.now() >= launch_until {
                NotStarted::Expired
            } else {
                NotStarted::Rejected
            }
        });
    let recorder = Arc::clone(&port);
    let owned = request.clone();
    let observed = observation.as_ref().ok().cloned();
    port.section(
        PortSection::write("runner.complete", request.operation.content.identity.agent_run_id),
        Box::new(move || {
            recorder.complete(
                &owned,
                observed.as_ref(),
                tree_stopped,
                cleanup,
                not_started,
                failure_code.as_deref(),
            )
        }),
    )
    .await
}

/// The launch window a finished preparation is allowed to dispatch in.
///
/// It is decided from the clock *after* the resource work, and it is bounded by both
/// ends that matter: the authority's own deadline, and the plan's own trust bound. So
/// it can neither be spent by preparation — which is the defect this replaces — nor
/// outlive the authority it belongs to.
///
/// `None` means there is no window left at all: the authority expired, or the plan did,
/// or the two together leave nothing. That is a refusal before any process exists, not
/// a launch that failed partway.
pub(crate) fn launch_window(
    now: UtcTimestamp,
    authority_deadline: UtcTimestamp,
    plan_valid_until: UtcTimestamp,
) -> Option<UtcTimestamp> {
    let bound = authority_deadline.min(plan_valid_until);
    let fresh = UtcTimestamp::from_value(
        now.value()
            + time::Duration::seconds(
                i64::try_from(budgets::LAUNCH_ELIGIBILITY.as_secs()).unwrap_or(i64::MAX),
            ),
    );
    let until = fresh.min(bound);
    (until > now).then_some(until)
}

/// One monotonic span in whole milliseconds, saturating rather than wrapping.
fn elapsed_ms(since: std::time::Instant) -> u64 {
    u64::try_from(since.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Only this newly allocated, exact identity is removed. Rust's Windows tree
/// removal does not follow junctions; the root and resolved parent are checked
/// again after the process Job has become empty and ACL handles are released.
pub(crate) fn remove_scratch_root(
    installation_root: &Path,
    binding: &ExecutionFileBinding,
) -> Result<()> {
    let root = crate::file_binding::verify(binding)?;
    let parent = aura_windows_platform::inspect_existing_path(
        &installation_root.join("execution-scratch"),
        false,
    )?;
    ensure!(
        root.info().is_directory
            && aura_windows_platform::final_path_within(&parent, root.info())
            && root.info().final_path.parent() == Some(parent.final_path.as_path()),
        "runner.scratch_cleanup_scope"
    );
    let path = root.info().final_path.clone();
    drop(root);
    std::fs::remove_dir_all(path)?;
    Ok(())
}

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "supervisor_wait_tests.rs"]
mod wait_tests;
