use super::super::AgentExecution;
use aura_desktop_service::DesktopControl;
use aura_domain_common::TurnTaskId;
use std::sync::Arc;

/// Called inside the existing Store section: the immutable run configuration owns the target.
pub(super) fn for_turn(
    service: &AgentExecution,
    db: &aura_persistence_core::Connection,
    turn: TurnTaskId,
) -> anyhow::Result<Arc<DesktopControl>> {
    let root = service.desktop.get().ok_or_else(|| anyhow::anyhow!(super::resolve::OFF))?;
    let turn = aura_persistence_v2::agent::turns::load(db, turn)?;
    let config = aura_persistence_v2::agent::application::configuration(db, turn.agent_run_id)?;
    root.select(config.request.computer_mode.unwrap_or_default())
        .map_err(|error| anyhow::anyhow!("computer.unavailable: {}", error.message))
}
