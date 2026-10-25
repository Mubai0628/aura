use super::*;

impl PersistenceCoordinator {
    /// The tool ids with an enabled instance from a current tool authority.
    ///
    /// # Why the Built-In release is part of this
    ///
    /// Installed tools use the coordinator's mutable instance records. Agent
    /// Built-Ins also have a Core-owned release verified against its Rust
    /// definitions; its file/Shell/task handlers are not the persisted signed
    /// registry-management release. Omitting that release falsely reports real
    /// Agent handlers such as `fs.write` as disabled.
    ///
    /// This is the same reasoning `aura_tool_registry::builder::ineligibility_reason`
    /// encodes: `candidate.instance.enabled` decides for **every** source kind, and
    /// the coordinator supplies the Built-In candidates. Reading the release here
    /// keeps the port on the current authorities. The journal's immutable
    /// execution records are deliberately not an enablement source. Exact
    /// definition/version/instance binding remains the dispatcher's responsibility.
    pub(super) fn enabled_tool_ids(
        &self,
        authority: &ToolAuthoritySnapshotV1,
    ) -> std::collections::BTreeSet<aura_domain_common::ToolId> {
        let mut enabled = authority
            .instances
            .iter()
            .filter(|instance| instance.enabled)
            .map(|instance| instance.tool_id)
            .collect::<std::collections::BTreeSet<_>>();
        // The Built-In release's instances, exactly as the registry builder adds
        // them. A release that cannot be validated contributes nothing, which
        // refuses Built-Ins rather than admitting them on an unverified release.
        if let Ok(entries) = self.validated_signed_builtin_release_authority() {
            enabled.extend(
                entries
                    .iter()
                    .filter(|entry| entry.instance.enabled)
                    .map(|entry| entry.definition.tool_id),
            );
        }
        if let Ok(release) = aura_tool_registry::agent_release::release() {
            for entry in &release.manifest().entries {
                let current = authority.instances.iter().find(|i| {
                    i.tool_instance_id == entry.instance.tool_instance_id
                        && i.tool_version == entry.instance.tool_version
                });
                enabled.remove(&entry.definition.tool_id);
                if current.map_or(entry.instance.enabled, |i| i.enabled) {
                    enabled.insert(entry.definition.tool_id);
                }
            }
        }
        enabled
    }
}
