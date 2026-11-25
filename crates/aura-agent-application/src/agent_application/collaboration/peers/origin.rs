use super::*;
use aura_agent_collaboration::{PeerOrigin, PeerSource};
use aura_persistence_v2::agent::{lineage, sender};

pub(super) fn from_turn(db: &Connection, turn: &TurnTask) -> StoreResult<PeerOrigin> {
    if let Some(received) = peer_input::delivery(db, turn)? {
        return refresh(db, &received.origin)?.ok_or(Error::Invalid(
            "peers.source_withdrawn: 原任务来源已无法核验或已取消；没有投递。",
        ));
    }
    if turn.lineage_resolution_required || !store::purpose(db, turn.agent_run_id)?.is_conversation()
    {
        return Err(Error::Conflict);
    }
    let current = lineage::load(db, turn.lineage_id)?;
    if current.session_id != turn.session_id
        || current.last_active_turn_task_id != turn.turn_task_id
        || current.state != TaskLineageState::Active
    {
        return Err(Error::Conflict);
    }
    check_root(db, &current)?;
    from_lineage(db, &current, turn.workspace_id)
}

/// A graph edge is not a history grant. Follow only the bound root lineage and
/// verified, active source ranges. Never include attachments or a different task.
pub(super) fn refresh(db: &Connection, frozen: &PeerOrigin) -> StoreResult<Option<PeerOrigin>> {
    let Some(current) = verified_lineage(db, frozen)? else { return Ok(None) };
    let origin = from_lineage(db, &current, frozen.workspace_id)?;
    Ok((!origin.sources.is_empty()).then_some(origin))
}

pub(super) fn verified_lineage(
    db: &Connection,
    frozen: &PeerOrigin,
) -> StoreResult<Option<TaskLineage>> {
    if aura_persistence_v2::workspace::session(db, frozen.session_id)?
        .is_none_or(|s| s.workspace_id != frozen.workspace_id)
    {
        return Ok(None);
    }
    let current = lineage::load(db, frozen.lineage_id)?;
    if current.session_id != frozen.session_id
        || current.root_user_event_id != frozen.root_event_id
        || current.revision < frozen.lineage_revision
        || matches!(current.state, TaskLineageState::CancelledByUser | TaskLineageState::Expired)
    {
        return Ok(None);
    }
    check_root(db, &current)?;
    Ok(Some(current))
}

fn check_root(db: &Connection, current: &TaskLineage) -> StoreResult<()> {
    let root = session::user_message(db, current.session_id, current.root_user_event_id)?;
    if root.origin != UserMessageOrigin::UserInput
        || sender::of_message(db, root.event_id)?.is_some()
    {
        return Err(Error::Conflict);
    }
    Ok(())
}

fn from_lineage(
    db: &Connection,
    current: &TaskLineage,
    workspace: WorkspaceId,
) -> StoreResult<PeerOrigin> {
    let mut sources = Vec::new();
    for entry in &current.sources {
        let excerpt = match source::authorized(db, current, entry)? {
            source::Material::Text(text) => source_preview(&text),
            source::Material::Absent => continue,
            source::Material::Sensitive => {
                "[Source text withheld: may contain credentials]".to_owned()
            }
        };
        sources.push(PeerSource {
            event_id: entry.user_event_id,
            content_hash: entry.content_hash.clone(),
            excerpt,
        });
        if sources.len() == 16 {
            break;
        }
    }
    Ok(PeerOrigin {
        workspace_id: workspace,
        session_id: current.session_id,
        lineage_id: current.lineage_id,
        root_event_id: current.root_user_event_id,
        lineage_revision: current.revision,
        sources,
    })
}
