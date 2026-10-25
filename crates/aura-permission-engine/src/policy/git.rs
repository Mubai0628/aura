//! The exact four Git actions use their existing capability baseline, including fetch in WR.
use super::*;

pub(super) fn mode_permits(
    input: &PolicyInput<'_>,
    capabilities: &[ResolvedCapability],
    networked: bool,
    controlled_web: bool,
) -> bool {
    let c = &input.operation.content;
    if crate::browser::shape(input) {
        return capabilities.iter().all(|cap| {
            aura_security_contract::browser_mode_covers(input.snapshot.mode, *cap, &c.resources)
        });
    }
    if built_in_memory(&c.source, &c.action_kind, &c.capabilities, &c.resources) {
        return capabilities.iter().all(|cap| match cap {
            ResolvedCapability::MemoryRead => true,
            ResolvedCapability::MemoryWrite => input.snapshot.mode != PermissionMode::ReadOnly,
            _ => false,
        });
    }
    if built_in_native_git(&c.source, &c.action_kind, &c.capabilities) {
        capabilities.iter().all(|cap| native_git_mode_covers(input.snapshot.mode, *cap))
    } else {
        crate::mode_ceiling::permits(
            c.identity.actor,
            input.snapshot.mode,
            capabilities,
            networked,
            controlled_web,
        )
    }
}

pub(super) fn network_allowed(input: &PolicyInput<'_>) -> bool {
    let c = &input.operation.content;
    input.snapshot.mode == PermissionMode::FullAccess
        || built_in_native_git(&c.source, &c.action_kind, &c.capabilities)
            && c.capabilities.iter().all(|cap| native_git_mode_covers(input.snapshot.mode, *cap))
}
