use super::*;
use aura_agent_collaboration::dispatch::controls;
use aura_persistence_v2::agent::lineage;

pub(in crate::agent_application) fn working_set(
    db: &Connection,
    turn: &TurnTask,
) -> StoreResult<Option<(EventId, String)>> {
    peer_input::delivery(db, turn)?
        .map(|delivery| {
            let origin = origin::refresh(db, &delivery.origin)?;
            Ok((delivery.id, peer_input::working_set(db, &delivery, origin.as_ref())?))
        })
        .transpose()
}

impl AgentApplication {
    pub(in crate::agent_application) fn peer_prompt_state(
        &self,
        db: &Connection,
        turn: &TurnTask,
        receiver: Option<&dispatch::Delivery>,
    ) -> anyhow::Result<Option<Value>> {
        if teams::member_by_child(db, turn.session_id)?.is_some() {
            return Ok(None);
        }
        let root = receiver.map_or(turn.root_user_event_id, |d| d.origin.root_event_id);
        let root = if receiver.is_none() && store::purpose(db, turn.agent_run_id)?.is_conversation()
        {
            lineage::load(db, turn.lineage_id)?.root_user_event_id
        } else {
            root
        };
        let deliveries = controls::recent_in_run(db, turn.workspace_id, turn.session_id, root)?;
        let has_links = directions(db, turn)?.1;
        if !has_links && receiver.is_none() && deliveries.is_empty() {
            return Ok(None);
        }
        let directory = if has_links {
            let projection = self.execution.persistence.workspace_session_projection()?;
            let mut directory =
                self.peer_directory(db, turn.session_id, turn.workspace_id, &projection.sessions)?;
            let neighbors = directory.peers.len();
            directory.peers.truncate(12);
            for peer in &mut directory.peers {
                peer.description = peer.description.chars().take(240).collect();
            }
            Some(
                serde_json::json!({"preview": directory, "neighbors": neighbors, "full_directory": "peers.list"}),
            )
        } else {
            None
        };
        let budget = controls::budget(db, turn.workspace_id, root)?;
        let states = deliveries
            .iter()
            .filter(|d| Some(d.id) != receiver.map(|d| d.id))
            .map(|d| self.peer_delivery_view(db, d, false))
            .collect::<StoreResult<Vec<_>>>()?;
        let scope_update = receiver.map(|d| {
            Ok::<_, Error>(match origin::refresh(db, &d.origin)? {
                None => serde_json::json!({ "status": "withdrawn", "instruction": "The original user's scope was withdrawn or is no longer available. Do not take further action on the old excerpts." }),
                Some(latest) if latest.sources != d.origin.sources => serde_json::json!({ "status": "revised", "current_requirements": latest, "instruction": "These active, verified sources replace the earlier excerpts, not the recipient's existing permission settings." }),
                Some(_) => serde_json::json!({ "status": "verified", "root_event_id": root }),
            })
        }).transpose()?;
        let mut seen = std::collections::BTreeSet::new();
        let repeated = deliveries.iter().any(|d| {
            !seen.insert((
                d.sender.session_id,
                d.target,
                d.kind == PeerDeliveryKind::Question,
                &d.input.task,
            ))
        });
        Ok(Some(serde_json::json!({
            "directory": directory,
            "budget": budget,
            "remaining": budget.limit.saturating_sub(budget.used),
            "recent_deliveries": states,
            "scope_update": scope_update,
            "repeat_observation": repeated.then_some("Matching instructions were sent more than once in this run. Check outcomes and progress before delegating again. This is not duplicate detection or a completion signal."),
        })))
    }
}
