//! Capability difference and explicit live authority decide selective managed stops.
use super::*;
use aura_execution_contract::OperationNetwork;
use aura_permission_engine::{mode_ceiling, policy};

pub(super) fn rank(mode: PermissionMode) -> u8 {
    match mode {
        PermissionMode::ReadOnly => 0,
        PermissionMode::WorkspaceReadWrite => 1,
        PermissionMode::FullAccess => 2,
    }
}

pub(crate) fn lost_default(
    operation: &SecuredOperation,
    before: PermissionMode,
    after: PermissionMode,
) -> bool {
    let c = &operation.content;
    let controlled = aura_security_contract::controlled_tool_network(
        &c.source,
        &c.action_kind,
        &c.capabilities,
        &c.resources,
    );
    c.capabilities.iter().any(|cap| {
        mode_ceiling::permits(c.identity.actor, before, &[*cap], false, controlled)
            && !mode_ceiling::permits(c.identity.actor, after, &[*cap], false, controlled)
    }) || c.network != OperationNetwork::None
        && mode_ceiling::permits(c.identity.actor, before, &[], true, controlled)
        && !mode_ceiling::permits(c.identity.actor, after, &[], true, controlled)
}

impl AgentExecution {
    /// The fresh default or a valid explicit approval still covers this immutable operation.
    pub(crate) fn operation_permitted_now(
        &self,
        operation: &SecuredOperation,
        snapshot: &PermissionSnapshot,
        mode: PermissionMode,
        revision: u64,
    ) -> anyhow::Result<bool> {
        let c = &operation.content;
        let protected = c.resources.iter().any(|r| {
            matches!(
                r,
                ResourceDescriptor::SystemPath { .. }
                    | ResourceDescriptor::SensitiveUserPath { .. }
            )
        });
        if !protected
            && mode_ceiling::permits(
                c.identity.actor,
                mode,
                &c.capabilities,
                c.network != OperationNetwork::None,
                aura_security_contract::controlled_tool_network(
                    &c.source,
                    &c.action_kind,
                    &c.capabilities,
                    &c.resources,
                ),
            )
        {
            return Ok(true);
        }
        let externally_covered = self.external_authority(operation);
        let external_grants = self.external_authority_slices(operation);
        if !c.capabilities.is_empty()
            && c.capabilities.iter().all(|capability| externally_covered.contains(capability))
        {
            return Ok(true);
        }
        let mut live = snapshot.clone();
        live.mode = mode;
        live.session_permission_revision = revision;
        let grants = aura_persistence_v2::security::grants_for_call(
            self.journal.lock()?.database.connection(),
            &operation.fingerprint,
            c.identity.session_id,
        )?;
        let input = policy::PolicyInput {
            network_caller: aura_security_contract::network_caller_for(&c.action_kind),
            operation,
            snapshot: &live,
            profile: SecurityProfile::for_permission_mode(mode),
            source_enabled: true,
            trust: &TrustState::SystemTrusted,
            source_fingerprint: c.source_revision_fingerprint.as_ref(),
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &grants,
            verified_external_grants: &external_grants,
            verified_external_capabilities: &externally_covered,
            now: UtcTimestamp::now(),
            expires_at: UtcTimestamp::now(),
            core_runtime_instance_id: Some(self.runtime),
        };
        Ok(aura_permission_engine::network::permitted_now(&input)
            || grants.iter().any(|grant| policy::explicit_grant_covers_operation(grant, &input)))
    }

    #[cfg(test)]
    pub(crate) async fn stop_permissions_removed(
        self: &Arc<Self>,
        session: SessionId,
        before: PermissionMode,
        deadline: std::time::Instant,
    ) -> anyhow::Result<()> {
        self.stop_authority_dependencies(session, Some(before), None, deadline).await
    }
}
