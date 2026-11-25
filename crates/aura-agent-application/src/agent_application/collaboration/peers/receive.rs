use super::*;
use aura_persistence_core::artifact;

impl AgentApplication {
    pub(in crate::agent_application) fn kick_peer(self: &Arc<Self>, target: SessionId) {
        let app = Arc::downgrade(self);
        tokio::spawn(async move {
            let Some(app) = app.upgrade() else { return };
            if let Err(error) = app.receive_peer(target).await {
                append_core_log(
                    "peers.receiver_unsettled",
                    serde_json::json!({"session":target,"error":error.to_string()}),
                );
            }
        });
    }

    pub(in crate::agent_application) async fn wake_peer_queues(
        self: &Arc<Self>,
    ) -> anyhow::Result<()> {
        let targets =
            self.store.read(StoreStep::operation("peers.queues"), dispatch::receivers).await?;
        for target in targets {
            self.kick_peer(target);
        }
        Ok(())
    }

    async fn receive_peer(self: &Arc<Self>, target: SessionId) -> anyhow::Result<()> {
        let pending = self
            .store
            .read(StoreStep::operation("peers.preflight"), move |db| {
                if !peer_input::idle(db, target)? {
                    return Ok(None);
                }
                let Some(id) = dispatch::pending(db, target)? else {
                    return Ok(None);
                };
                Ok(Some((id, dispatch::retry_deadline(db, id)?)))
            })
            .await?;
        let Some((id, retry_at)) = pending else { return Ok(()) };
        if let Some(at) = retry_at.filter(|at| *at > UtcTimestamp::now()) {
            self.schedule_peer_retry(target, at);
            return Ok(());
        }
        let lease = self.execution.persistence.permission_start_lease(target).await;
        let primary = self.execution.persistence.workspace_session_projection()?;
        let Some(summary) = primary.sessions.iter().find(|s| s.session_id == target) else {
            return Ok(());
        };
        let Some(binding) = self.execution.persistence.session_model_binding(target)? else {
            self.record_peer_retry(target, id, "model_unavailable").await?;
            return Ok(());
        };
        let selected = binding.model_profile_id;
        if !self
            .p05
            .model_config
            .snapshot()?
            .registry
            .models
            .get(&selected)
            .is_some_and(|m| m.enabled)
        {
            self.record_peer_retry(target, id, "model_unavailable").await?;
            return Ok(());
        }
        let workspace = summary.workspace_id;
        let at = UtcTimestamp::now();
        let expected_turn = TurnTaskId::new();
        let bundle = aura_prompt_runtime::freeze_prompt_bundle(
            &aura_prompt_runtime::ValidatedPromptBundle::embedded()?,
            &self.settings.prompt_snapshot()?,
            expected_turn,
            env!("CARGO_PKG_VERSION"),
            at,
        )?;
        let tool_settings = self.settings.tool_settings_snapshot()?;
        let result = self
            .store
            .write(StoreStep::operation("peers.claim"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    if teams::member_by_child(tx, target)?.is_some()
                        || !peer_input::idle(tx, target)?
                        || dispatch::pending(tx, target)? != Some(id)
                        || !dispatch::retry_due(tx, id)?
                    {
                        return Ok(None);
                    }
                    let delivery = dispatch::load(tx, id)?.ok_or(Error::Conflict)?;
                    if delivery.origin.workspace_id != workspace
                        || origin::refresh(tx, &delivery.origin)?.is_none()
                    {
                        return Ok(None);
                    }
                    for part in &bundle.artifacts {
                        artifact::put(
                            tx,
                            part.artifact_id,
                            &bundle.snapshot.id.to_string(),
                            &part.media_type,
                            &part.bytes,
                            at,
                        )?;
                    }
                    aura_persistence_v2::context::insert_bundle(tx, target, &bundle.snapshot)?;
                    let turn = peer_input::start(tx, &delivery, bundle.snapshot.id, at)?;
                    let config = store::RunConfiguration {
                        request: AgentStartRequest {
                            computer_mode: None,
                            session_id: target,
                            model_profile_id: selected,
                            text: String::new(),
                            mode: AgentTaskMode::Normal,
                            goal_budget: None,
                            explicit_backend_id: None,
                            skill_ids: Vec::new(),
                            reasoning_effort: None,
                            speed_mode: None,
                            attachments: Vec::new(),
                            references: Vec::new(),
                        },
                        tool_settings,
                        purpose: AgentTaskPurpose::PeerDispatch,
                    };
                    store::configure(
                        tx,
                        turn.agent_run_id,
                        &config,
                        CommandInvocationId::new(),
                        at,
                    )?;
                    Ok(Some(turn.agent_run_id))
                })
            })
            .await;
        let run = match result {
            Ok(run) => run,
            Err(error @ StoreError::Unknown { .. }) => {
                // The write may already have bound exactly this receiver turn. Do not resubmit
                // the claim. If the binding is known, only that committed pending run can launch.
                let matched = self
                    .store
                    .read(StoreStep::operation("peers.claim.reconcile"), move |db| {
                        let Some(run) = dispatch::load(db, id)?.and_then(|d| d.target_run) else {
                            return Ok(None);
                        };
                        let turn = turns::load(db, turns::load_run(db, run)?.turn_task_id)?;
                        Ok((turn.turn_task_id == expected_turn
                            && turn.state == TurnTaskState::Pending)
                            .then_some(run))
                    })
                    .await?;
                if matched.is_none() {
                    return Err(error.into());
                }
                matched
            }
            Err(error) => return Err(error.into()),
        };
        drop(lease);
        if let Some(run) = run {
            self.launch(run)?;
        }
        Ok(())
    }

    fn schedule_peer_retry(self: &Arc<Self>, target: SessionId, deadline: UtcTimestamp) {
        let app = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            let millis = (deadline.value() - UtcTimestamp::now().value())
                .whole_milliseconds()
                .clamp(25, 60_000) as u64;
            tokio::time::sleep(std::time::Duration::from_millis(millis)).await;
            // Recheck the deadline, queue head and idle state before claiming.
            // This timer never owns or replays a claimed run.
            if let Some(app) = app.upgrade() {
                app.kick_peer(target);
            }
        });
        self.peer_retry_timers.install(target, task);
    }

    async fn record_peer_retry(
        self: &Arc<Self>,
        target: SessionId,
        delivery: EventId,
        reason: &'static str,
    ) -> anyhow::Result<()> {
        let recorded = self
            .store
            .write(StoreStep::operation("peers.retry"), move |journal| {
                journal.database.unit_of_work(|tx| {
                    if dispatch::pending(tx, target)? != Some(delivery) {
                        return Ok(None);
                    }
                    // Concurrent wake hints cannot advance the same due retry twice.
                    if dispatch::retry_due(tx, delivery)? {
                        dispatch::record_retry(tx, delivery, reason)?;
                    }
                    dispatch::retry_deadline(tx, delivery)
                })
            })
            .await;
        let deadline = match recorded {
            Ok(deadline) => deadline,
            Err(error @ StoreError::Unknown { .. }) => {
                let known = self
                    .store
                    .read(StoreStep::operation("peers.retry.reconcile"), move |db| {
                        dispatch::retry_deadline(db, delivery)
                    })
                    .await?;
                if known.is_none() {
                    return Err(error.into());
                }
                known
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(deadline) = deadline {
            self.schedule_peer_retry(target, deadline);
        }
        Ok(())
    }
}
