//! The model-facing adapter takes its identity from the verified execution port.
use super::*;
use aura_agent_collaboration::{
    PeerAskInput, PeerAvailability, PeerDeliveryKind, PeerDeliveryState, PeerDeliveryView,
    PeerDirectory, PeerDirectoryEntry, PeerDispatchInput, PeerDispatchReceipt, PeerLookup,
    PeerLookupResult, PeerSender, PeerSourceContent, PeerSourceInput, PeersListInput, dispatch,
    graph, profile,
};
use aura_agent_domain::{AgentPeerResultKind, AgentPeerResultNotice, AgentPeerResultState};
use aura_agent_team::{TeamCall, store as teams};
use aura_core_broker::agent_store::StoreStep;
use aura_persistence_core::{Connection, PersistenceCoreError as Error, StoreResult};
use aura_persistence_v2::agent::{peer_input, session};
use aura_workspace_domain::SessionSummary;
use serde_json::Value;
mod context;
mod manage;
mod origin;
mod receive;
mod source;
pub(in crate::agent_application) use context::working_set;

const INELIGIBLE: &str =
    "peers.target_ineligible: 目标已不在本工作区或不是普通会话；没有接受派发。";

fn peer_candidate(
    db: &Connection,
    id: SessionId,
    summaries: &[SessionSummary],
    workspace: WorkspaceId,
) -> StoreResult<bool> {
    Ok(summaries.iter().any(|s| s.session_id == id && s.workspace_id == workspace)
        && aura_persistence_v2::workspace::session(db, id)?
            .is_some_and(|s| s.workspace_id == workspace)
        && teams::member_by_child(db, id)?.is_none())
}

pub(in crate::agent_application) fn directions(
    db: &Connection,
    turn: &TurnTask,
) -> StoreResult<(bool, bool)> {
    let graph = graph::load(db, turn.session_id, turn.workspace_id)?;
    let outgoing = graph.edges.iter().any(|e| e.source_session_id == turn.session_id);
    let adjacent = outgoing || graph.edges.iter().any(|e| e.target_session_id == turn.session_id);
    Ok((outgoing, adjacent))
}

impl AgentApplication {
    pub(in crate::agent_application) async fn peer_tool(
        self: Arc<Self>,
        call: TeamCall,
    ) -> Result<Value, String> {
        let id = call.tool_call_id;
        let writes = matches!(call.tool.as_str(), "peers.dispatch" | "peers.ask");
        self.invoke_peer(call).await.map_err(|error| {
            if writes && matches!(error.downcast_ref::<StoreError>(), Some(StoreError::Unknown { .. })) {
                format!("peers.outcome_unknown: 未确认接收结果。请先用 peers.status 按原调用 {id} 对账；不要重新投递。")
            } else if let Some(StoreError::Failed { error: Error::Invalid(reason), .. }) = error.downcast_ref::<StoreError>() {
                (*reason).to_owned()
            } else {
                format!("peers.operation_failed: 无法读取或处理协作请求：{error}")
            }
        })
    }

    async fn invoke_peer(self: &Arc<Self>, call: TeamCall) -> anyhow::Result<Value> {
        let sender = PeerSender {
            session_id: call.session_id,
            turn_task_id: call.turn_task_id,
            tool_call_id: call.tool_call_id,
        };
        let primary = self.execution.persistence.workspace_session_projection()?;
        let workspace = primary
            .sessions
            .iter()
            .find(|s| s.session_id == sender.session_id)
            .ok_or_else(|| anyhow::anyhow!(INELIGIBLE))?
            .workspace_id;
        let sessions = primary.sessions;
        Ok(match call.tool.as_str() {
            "peers.list" => {
                let _: PeersListInput = serde_json::from_value(call.arguments)?;
                let app = Arc::clone(self);
                let directory = self
                    .store
                    .run_critical(
                        StoreStep::operation("peers.list"),
                        StoreAccess::Read,
                        move || {
                            let journal = app.execution.journal.lock()?;
                            app.peer_directory(
                                journal.database.connection(),
                                sender.session_id,
                                workspace,
                                &sessions,
                            )
                        },
                    )
                    .await?;
                serde_json::to_value(directory)?
            }
            "peers.dispatch" | "peers.ask" => {
                let (kind, input): (PeerDeliveryKind, PeerDispatchInput) =
                    if call.tool == "peers.ask" {
                        (
                            PeerDeliveryKind::Question,
                            serde_json::from_value::<PeerAskInput>(call.arguments)?.into(),
                        )
                    } else {
                        (PeerDeliveryKind::Dispatch, serde_json::from_value(call.arguments)?)
                    };
                anyhow::ensure!(
                    aura_secret_scanner::detect_text(&input.task).is_none(),
                    "peers.secret: 内容可能含有凭据；未接受投递。"
                );
                let app = Arc::clone(self);
                let (receipt, targets) = self
                    .store
                    .write(StoreStep::operation("peers.dispatch"), move |journal| {
                        journal.database.unit_of_work(|tx| {
                            let (ids, fresh) =
                                match dispatch::receipt_ids(tx, sender, &input, kind)? {
                                    Some(ids) => (ids, false),
                                    None => {
                                        let source = turns::load(tx, sender.turn_task_id)?;
                                        if source.session_id != sender.session_id
                                            || source.workspace_id != workspace
                                            || source.state.terminal()
                                            || session::load(tx, source.session_id)?.active_turn
                                                != Some(source.turn_task_id)
                                            || !peer_candidate(
                                                tx,
                                                sender.session_id,
                                                &sessions,
                                                workspace,
                                            )?
                                        {
                                            return Err(Error::Conflict);
                                        }
                                        for id in &input.targets {
                                            if !peer_candidate(tx, *id, &sessions, workspace)? {
                                                return Err(Error::Invalid(INELIGIBLE));
                                            }
                                        }
                                        let origin = origin::from_turn(tx, &source)?;
                                        (
                                            dispatch::accept(
                                                tx,
                                                sender,
                                                &input,
                                                kind,
                                                &origin,
                                                UtcTimestamp::now(),
                                            )?,
                                            true,
                                        )
                                    }
                                };
                            let first = ids.first().ok_or(Error::Conflict)?;
                            let first = dispatch::load(tx, *first)?.ok_or(Error::Conflict)?;
                            let (used, limit) = dispatch::budget(tx, &first.origin)?;
                            let previous = used.saturating_sub(ids.len() as u32);
                            let (deliveries, targets) = ids
                                .into_iter()
                                .map(|id| {
                                    let info = dispatch::load(tx, id)?.ok_or(Error::Conflict)?;
                                    Ok((app.peer_delivery_view(tx, &info, false)?, info.target))
                                })
                                .collect::<StoreResult<Vec<_>>>()?
                                .into_iter()
                                .unzip::<_, _, Vec<_>, Vec<_>>();
                            Ok((
                                PeerDispatchReceipt {
                                    deliveries,
                                    used,
                                    limit,
                                    nearing_limit: fresh
                                        && used * 5 >= limit * 4
                                        && previous * 5 < limit * 4,
                                },
                                targets,
                            ))
                        })
                    })
                    .await?;
                // Waking is only a hint. A lost hint or a failed receiver does not undo acceptance.
                for target in targets {
                    self.kick_peer(target);
                }
                serde_json::to_value(receipt)?
            }
            "peers.status" | "peers.result" => {
                let lookup: PeerLookup = serde_json::from_value(call.arguments)?;
                let include_result = call.tool == "peers.result";
                let app = Arc::clone(self);
                let result = self
                    .store
                    .read(StoreStep::operation("peers.reconcile"), move |db| {
                        source::lookup(db, sender.session_id, workspace, lookup, |d| {
                            app.peer_delivery_view(db, d, include_result)
                        })
                    })
                    .await?;
                serde_json::to_value(result)?
            }
            "peers.read_source" => {
                let input: PeerSourceInput = serde_json::from_value(call.arguments)?;
                let content = self
                    .store
                    .read(StoreStep::operation("peers.source"), move |db| {
                        source::read(db, sender, workspace, &input)
                    })
                    .await?;
                serde_json::to_value(content)?
            }
            _ => anyhow::bail!("peers.unknown_tool"),
        })
    }

    fn peer_directory(
        &self,
        db: &Connection,
        sender: SessionId,
        workspace: WorkspaceId,
        summaries: &[SessionSummary],
    ) -> anyhow::Result<PeerDirectory> {
        anyhow::ensure!(peer_candidate(db, sender, summaries, workspace)?, INELIGIBLE);
        let graph = graph::load(db, sender, workspace)?;
        let mut peers = Vec::new();
        let mut neighbors = std::collections::BTreeMap::new();
        for edge in graph.edges {
            if edge.source_session_id == sender {
                neighbors.insert(edge.target_session_id, true);
            } else if edge.target_session_id == sender {
                neighbors.entry(edge.source_session_id).or_insert(false);
            }
        }
        let requires_confirmation = dispatch::review::policy(db)?.required;
        for (id, can_dispatch) in neighbors {
            if !peer_candidate(db, id, summaries, workspace)? {
                continue;
            }
            let Some(summary) = summaries.iter().find(|s| s.session_id == id) else {
                continue;
            };
            let profile = profile::load(db, id, workspace, &summary.title)?;
            use peer_input::Blockage;
            let (availability, reason) = match peer_input::blockage(db, id)? {
                Some(wait @ (Blockage::ActiveTurn | Blockage::UserInput)) => {
                    (PeerAvailability::Busy, Some(wait.message()))
                }
                Some(wait) => (PeerAvailability::Paused, Some(wait.message())),
                None if !self.peer_has_model(id)? => {
                    (PeerAvailability::ModelUnavailable, Some(MODEL_UNAVAILABLE))
                }
                None => (PeerAvailability::Ready, None),
            };
            peers.push(PeerDirectoryEntry {
                session_id: id,
                name: profile.name,
                description: profile.description,
                profile_revision: profile.revision,
                availability,
                availability_reason: reason.map(str::to_owned),
                requires_confirmation,
                can_dispatch,
            });
        }
        peers.sort_by_key(|peer| peer.session_id);
        Ok(PeerDirectory { graph_id: graph.graph_id, graph_revision: graph.revision, peers })
    }

    fn peer_has_model(&self, target: SessionId) -> anyhow::Result<bool> {
        let models = self.p05.model_config.snapshot()?;
        Ok(self.execution.persistence.session_model_binding(target)?.is_some_and(|binding| {
            models.registry.models.get(&binding.model_profile_id).is_some_and(|model| model.enabled)
        }))
    }

    fn peer_delivery_view(
        &self,
        db: &Connection,
        d: &dispatch::Delivery,
        include_result: bool,
    ) -> StoreResult<PeerDeliveryView> {
        let mut view = delivery_view(db, d, include_result)?;
        if view.state == PeerDeliveryState::Queued && view.reason.is_none() {
            let reason = if !self.peer_has_model(d.target).map_err(|_| Error::Conflict)? {
                MODEL_UNAVAILABLE
            } else if d.kind == PeerDeliveryKind::Question {
                "问题仍在处理，可按原投递 ID 查询。"
            } else {
                "原投递正在等待领取。"
            };
            view.reason = Some(reason.into());
        }
        Ok(view)
    }
}

const MODEL_UNAVAILABLE: &str = "目标会话未绑定可用模型，绑定后可继续原投递。";

pub(super) fn graph_activity(
    db: &Connection,
    graph: &aura_agent_collaboration::AgentGraph,
) -> StoreResult<Vec<aura_agent_collaboration::PeerEdgeActivity>> {
    use aura_agent_collaboration::{AgentGraphEdge, PeerEdgeActivity};
    use std::collections::{BTreeMap, BTreeSet};
    if graph.edges.is_empty() {
        return Ok(Vec::new());
    }
    let linked = graph.edges.iter().copied().collect::<BTreeSet<_>>();
    let mut activities = BTreeMap::new();
    for &edge in &graph.edges {
        if let Some(d) = dispatch::latest_between(db, graph.workspace_id, edge)? {
            activities.insert(edge, delivery_view(db, &d, false)?.state);
        }
    }
    // The active turn takes precedence over newer queued deliveries on the same connection.
    for &target in &graph.sessions {
        let Some(active) = session::load(db, target)?.active_turn else { continue };
        let turn = turns::load(db, active)?;
        if turn.state.terminal() || turn.workspace_id != graph.workspace_id {
            continue;
        }
        let Some(received) = peer_input::delivery(db, &turn)? else { continue };
        let source = received.sender.session_id;
        let edge = AgentGraphEdge { source_session_id: source, target_session_id: target };
        let reverse = AgentGraphEdge { source_session_id: target, target_session_id: source };
        if linked.contains(&edge)
            || (received.kind == PeerDeliveryKind::Question && linked.contains(&reverse))
        {
            activities.insert(edge, PeerDeliveryState::Started);
        }
    }
    Ok(activities
        .into_iter()
        .map(|(edge, state)| PeerEdgeActivity { edge, state, paused_by_global_switch: false })
        .collect())
}

fn delivery_view(
    db: &Connection,
    d: &dispatch::Delivery,
    include_result: bool,
) -> StoreResult<PeerDeliveryView> {
    use PeerDeliveryState as State;
    let mut view = PeerDeliveryView {
        delivery_id: d.id,
        kind: d.kind,
        source_session_id: d.sender.session_id,
        target_session_id: d.target,
        target_run_id: d.target_run,
        state: State::Queued,
        reason: None,
        result: None,
        sources: Vec::new(),
        next_source_cursor: None,
        source_catalog_issue: None,
    };
    if let Some(run) = d.target_run {
        let turn = turns::load(db, turns::load_run(db, run)?.turn_task_id)?;
        if turn.root_user_event_id != d.id || turn.session_id != d.target {
            return Err(Error::Conflict);
        }
        view.state = match turn.state {
            TurnTaskState::Completed { final_report } => {
                if include_result {
                    view.result = Some(final_report);
                }
                State::Completed
            }
            TurnTaskState::Failed { reason } => {
                view.reason = Some(reason);
                State::Failed
            }
            TurnTaskState::Cancelled => State::Cancelled,
            TurnTaskState::RecoveryRequired { reason } => {
                view.reason = Some(format!("{reason}；原轮次结果未知，请先对账，不要重复派发。"));
                State::Unknown
            }
            _ => State::Started,
        };
    } else if d.cancelled {
        view.state = State::Cancelled;
    } else if dispatch::review::waiting(db, d.id)? {
        view.reason = Some("等待接收方用户确认。".into());
    } else if aura_persistence_v2::workspace::session(db, d.target)?
        .is_none_or(|session| session.workspace_id != d.origin.workspace_id)
    {
        view.reason = Some("目标会话已删除或不在原工作区，可取消原投递。".into());
    } else if origin::verified_lineage(db, &d.origin)?.is_none() {
        view.reason = Some("原任务来源不可核验，可取消原投递。".into());
    } else {
        view.reason = peer_input::blockage(db, d.target)?.map(|wait| wait.message().into());
    }
    Ok(view)
}

/// Result notices for requestors. The body stays behind the receipt; this
/// read starts no turn and spends no collaboration unit.
pub(in crate::agent_application) fn unseen_results(
    db: &Connection,
) -> StoreResult<Vec<AgentPeerResultNotice>> {
    let mut notices = Vec::new();
    for (requester, id) in dispatch::unseen_results(db)? {
        let Some(d) = dispatch::load(db, id)? else { continue };
        let view = delivery_view(db, &d, false)?;
        let state = match view.state {
            PeerDeliveryState::Completed => AgentPeerResultState::Completed,
            PeerDeliveryState::Failed => AgentPeerResultState::Failed,
            PeerDeliveryState::Cancelled => AgentPeerResultState::Cancelled,
            PeerDeliveryState::Unknown => AgentPeerResultState::Unknown,
            _ => continue,
        };
        let target_title = aura_persistence_v2::workspace::session(db, d.target)?
            .map(|session| session.title)
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| "对端会话".to_owned());
        notices.push(AgentPeerResultNotice {
            session_id: requester,
            delivery_id: id,
            kind: match d.kind {
                PeerDeliveryKind::Dispatch => AgentPeerResultKind::Dispatch,
                PeerDeliveryKind::Question => AgentPeerResultKind::Question,
            },
            target_session_id: d.target,
            target_title,
            state,
        });
    }
    Ok(notices)
}
