use super::*;
use aura_agent_collaboration::{
    PeerActiveBudget, PeerMailbox, PeerMailboxEntry, PeerMailboxRequest, PeerMailboxView,
    PeerManageAction, PeerManageRequest,
    dispatch::{controls, review},
};
use aura_persistence_v2::workspace;

fn scope(db: &Connection, session: SessionId) -> StoreResult<WorkspaceId> {
    if teams::member_by_child(db, session)?.is_some() {
        return Err(Error::Conflict);
    }
    Ok(workspace::session(db, session)?.ok_or(Error::Conflict)?.workspace_id)
}

fn authorized_delivery(
    db: &Connection,
    workspace: WorkspaceId,
    session: SessionId,
    id: EventId,
) -> StoreResult<dispatch::Delivery> {
    let d = dispatch::load(db, id)?.ok_or(Error::Conflict)?;
    if d.origin.workspace_id != workspace || (d.target != session && d.sender.session_id != session)
    {
        return Err(Error::Conflict);
    }
    Ok(d)
}

fn mailbox(
    db: &Connection,
    input: PeerMailboxRequest,
    view: impl Fn(&dispatch::Delivery) -> StoreResult<PeerDeliveryView>,
) -> StoreResult<PeerMailbox> {
    if let Some(session) = input.session_id {
        scope(db, session)?;
    }
    let (entries, before) = if input.view == PeerMailboxView::Settings {
        (Vec::new(), None)
    } else {
        let session = input.session_id.ok_or(Error::Conflict)?;
        mailbox_entries(db, session, input, view)?
    };
    let active_budget = if input.view == PeerMailboxView::Active {
        active_budget(db, input.session_id.ok_or(Error::Conflict)?, &entries)?
    } else {
        None
    };
    Ok(PeerMailbox {
        collaboration_enabled: true,
        budget_default: controls::default_budget(db)?,
        review_policy: review::policy(db)?,
        entries,
        before,
        active_budget,
    })
}

fn active_budget(
    db: &Connection,
    session_id: SessionId,
    entries: &[PeerMailboxEntry],
) -> StoreResult<Option<PeerActiveBudget>> {
    let Some(active) = session::load(db, session_id)?.active_turn else { return Ok(None) };
    let turn = turns::load(db, active)?;
    if !store::purpose(db, turn.agent_run_id)?.is_conversation() || turn.lineage_resolution_required
    {
        return Ok(None);
    }
    let lineage = aura_persistence_v2::agent::lineage::load(db, turn.lineage_id)?;
    let budget = controls::budget(db, turn.workspace_id, lineage.root_user_event_id)?;
    if budget.stopped
        || budget.used < budget.limit
        || entries
            .iter()
            .any(|entry| entry.manages_run && entry.budget.root_event_id == budget.root_event_id)
    {
        return Ok(None);
    }
    Ok(controls::latest_outgoing_in_run(db, turn.workspace_id, session_id, budget.root_event_id)?
        .map(|delivery_id| PeerActiveBudget { delivery_id, budget }))
}

fn mailbox_entries(
    db: &Connection,
    session: SessionId,
    input: PeerMailboxRequest,
    view: impl Fn(&dispatch::Delivery) -> StoreResult<PeerDeliveryView>,
) -> StoreResult<(Vec<PeerMailboxEntry>, Option<EventId>)> {
    let workspace = scope(db, session)?;
    let (deliveries, before) = controls::mailbox(
        db,
        session,
        workspace,
        input.before,
        input.view == PeerMailboxView::Active,
    )?;
    let entries = deliveries
        .into_iter()
        .map(|d| {
            Ok(PeerMailboxEntry {
                delivery: view(&d)?,
                budget: controls::budget(db, workspace, d.origin.root_event_id)?,
                manages_run: session == d.origin.session_id,
                review_text: if d.target == session && !d.cancelled && review::waiting(db, d.id)? {
                    Some(d.input.task.clone())
                } else {
                    None
                },
            })
        })
        .collect::<StoreResult<_>>()?;
    Ok((entries, before))
}

fn cancel(
    tx: &aura_persistence_core::Transaction<'_>,
    delivery: &dispatch::Delivery,
    at: UtcTimestamp,
) -> StoreResult<Option<AgentRunRequest>> {
    if controls::cancel_queued(tx, delivery)? {
        return Ok(None);
    }
    let Some(run) = delivery.target_run else {
        return Ok(None);
    };
    let view = delivery_view(tx, delivery, false)?;
    // A reconciliation-required run may already have performed effects. Do not
    // turn it into an apparent cancellation or relaunch it to stop it.
    if view.state != PeerDeliveryState::Started {
        return Ok(None);
    }
    store::request_cancel(tx, run, at)?;
    Ok(Some(AgentRunRequest { session_id: delivery.target, agent_run_id: run }))
}

impl AgentApplication {
    pub(in crate::agent_application) async fn peer_mailbox_route(
        self: &Arc<Self>,
        method: &str,
        params: Value,
    ) -> anyhow::Result<Value> {
        let input = match method {
            "agent.team.peers.get" => serde_json::from_value(params)?,
            "agent.team.peers.manage" => {
                let input: PeerManageRequest = serde_json::from_value(params)?;
                let session = input.session_id;
                if let Some(session) = session {
                    self.ensure_session(session).await?;
                } else {
                    anyhow::ensure!(
                        input.view == PeerMailboxView::Settings
                            && matches!(
                                input.action,
                                PeerManageAction::SetDefault { .. }
                                    | PeerManageAction::SetReview { .. }
                            ),
                        "peers.session_required"
                    );
                }
                let stops = self
                    .store
                    .write(StoreStep::operation("peers.manage"), move |journal| {
                        journal.database.unit_of_work(|tx| {
                            let scoped = || -> StoreResult<_> {
                                let session = session.ok_or(Error::Conflict)?;
                                Ok((session, scope(tx, session)?))
                            };
                            let at = UtcTimestamp::now();
                            let mut stops = Vec::new();
                            match input.action {
                                PeerManageAction::SetDefault { expected_revision, limit } => {
                                    controls::set_default(tx, expected_revision, limit)?;
                                }
                                PeerManageAction::Increase {
                                    delivery_id,
                                    expected_revision,
                                    limit,
                                } => {
                                    let (session, workspace) = scoped()?;
                                    let d =
                                        authorized_delivery(tx, workspace, session, delivery_id)?;
                                    if d.origin.session_id != session {
                                        return Err(Error::Conflict);
                                    }
                                    controls::increase(tx, &d.origin, expected_revision, limit)?;
                                }
                                PeerManageAction::StopRun { delivery_id, expected_revision } => {
                                    let (session, workspace) = scoped()?;
                                    let d =
                                        authorized_delivery(tx, workspace, session, delivery_id)?;
                                    if d.origin.session_id != session {
                                        return Err(Error::Conflict);
                                    }
                                    for item in controls::stop(tx, &d.origin, expected_revision)? {
                                        stops.extend(cancel(tx, &item, at)?);
                                    }
                                }
                                PeerManageAction::Cancel { delivery_id } => {
                                    let (session, workspace) = scoped()?;
                                    let d =
                                        authorized_delivery(tx, workspace, session, delivery_id)?;
                                    stops.extend(cancel(tx, &d, at)?);
                                }
                                PeerManageAction::MarkResultSeen { delivery_id } => {
                                    let (session, workspace) = scoped()?;
                                    let d =
                                        authorized_delivery(tx, workspace, session, delivery_id)?;
                                    let view = delivery_view(tx, &d, false)?;
                                    if d.sender.session_id != session
                                        || !matches!(
                                            view.state,
                                            PeerDeliveryState::Completed
                                                | PeerDeliveryState::Failed
                                                | PeerDeliveryState::Cancelled
                                                | PeerDeliveryState::Unknown
                                        )
                                    {
                                        return Err(Error::Conflict);
                                    }
                                    dispatch::mark_result_seen(tx, &d, session, at)?;
                                }
                                PeerManageAction::WakeQueued => {}
                                PeerManageAction::SetReview { expected_revision, required } => {
                                    if session.is_some() {
                                        return Err(Error::Invalid(
                                            "peers.review_requires_global_settings",
                                        ));
                                    }
                                    review::set_policy(tx, expected_revision, required)?
                                }
                                PeerManageAction::Release { delivery_id, revised_text } => {
                                    let (session, workspace) = scoped()?;
                                    let d =
                                        authorized_delivery(tx, workspace, session, delivery_id)?;
                                    if revised_text.as_ref().is_some_and(|text| {
                                        aura_secret_scanner::detect_text(text).is_some()
                                    }) {
                                        return Err(Error::Invalid(
                                            "peers.review_secret: 编辑内容可能包含凭据；未放行。",
                                        ));
                                    }
                                    review::release(tx, &d, session, revised_text.as_deref(), at)?;
                                }
                            }
                            Ok(stops)
                        })
                    })
                    .await?;
                for request in &stops {
                    let _ = self.supervisor.request_cancel(request.agent_run_id);
                }
                let mut stops = tokio::task::JoinSet::from_iter(stops.into_iter().map(|request| {
                    let app = Arc::clone(self);
                    async move { app.cancel(&request).await.map(|_| ()) }
                }));
                // Drain all scoped stops, even if one needs additional reconciliation.
                let mut failure = None;
                while let Some(outcome) = stops.join_next().await {
                    if let Err(error) =
                        outcome.map_err(anyhow::Error::from).and_then(std::convert::identity)
                    {
                        failure.get_or_insert(error);
                    }
                }
                if let Some(failure) = failure {
                    return Err(failure);
                }
                if let Some(session) = session {
                    self.kick_peer(session);
                }
                PeerMailboxRequest { session_id: session, before: None, view: input.view }
            }
            _ => anyhow::bail!("peers.unknown_command"),
        };
        if let Some(session) = input.session_id {
            self.ensure_session(session).await?;
        }
        let app = Arc::clone(self);
        let response = self
            .store
            .run_critical(StoreStep::operation("peers.mailbox"), StoreAccess::Read, move || {
                let journal = app.execution.journal.lock()?;
                let db = journal.database.connection();
                Ok(mailbox(db, input, |d| {
                    app.peer_delivery_view(
                        db,
                        d,
                        input.view == PeerMailboxView::All
                            && input.session_id == Some(d.sender.session_id),
                    )
                })?)
            })
            .await?;
        Ok(serde_json::to_value(response)?)
    }
}
