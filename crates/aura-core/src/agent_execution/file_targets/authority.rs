use super::*;

impl AgentExecution {
    pub(in crate::agent_execution) fn external_authority_slices(
        &self,
        operation: &SecuredOperation,
    ) -> Vec<aura_security_contract::VerifiedExternalCapability> {
        let covered = self.external_authority(operation);
        let mut slices = Vec::new();
        for resource in &operation.content.resources {
            let ResourceDescriptor::ExternalGrant { grant_id } = resource else { continue };
            let Ok(record) = self.external_grants.record(*grant_id) else { continue };
            for capability in &covered {
                if self.external_grants.covers_capabilities(
                    *grant_id,
                    record.session_id,
                    &[*capability],
                ) {
                    slices.push(aura_security_contract::VerifiedExternalCapability {
                        capability: *capability,
                        grant_id: *grant_id,
                        revalidation_revision: record.revalidation_revision,
                    });
                }
            }
        }
        slices
    }

    pub(in crate::agent_execution) fn external_authority(
        &self,
        operation: &SecuredOperation,
    ) -> Vec<ResolvedCapability> {
        let Some(session) = operation.content.identity.session_id else { return vec![] };
        let enumerates =
            matches!(operation.content.action_kind.as_str(), "fs.list" | "fs.find" | "fs.search");
        let mut covered = Vec::new();
        for capability in &operation.content.capabilities {
            let verified = operation.content.resources.iter().any(|resource| match resource {
                ResourceDescriptor::ExternalGrant { grant_id } => {
                    self.external_grants.covers_capabilities(*grant_id, session, &[*capability])
                        && (!enumerates
                            || operation
                                .content
                                .action
                                .get(if operation.content.action_kind == "fs.search" {
                                    "root"
                                } else {
                                    "path"
                                })
                                .and_then(serde_json::Value::as_str)
                                .and_then(|path| {
                                    aura_domain_common::WorkspacePath::parse(path).ok()
                                })
                                .is_some_and(|path| {
                                    self.external_grants.record(*grant_id).ok().is_some_and(
                                        |record| {
                                            crate::external_grants::contents_are_authorized(
                                                &record, &path,
                                            )
                                        },
                                    )
                                }))
                }
                ResourceDescriptor::ExternalPath { canonical_path, .. }
                | ResourceDescriptor::SystemPath { canonical_path, .. }
                | ResourceDescriptor::SensitiveUserPath { canonical_path, .. } => {
                    let Some(workspace) = operation.content.identity.workspace_id else {
                        return false;
                    };
                    let path = std::path::Path::new(&canonical_path.display);
                    if aura_windows_platform::inspect_existing_path(path, true)
                        .ok()
                        .is_some_and(|info| self.control_resources.contains(&info))
                    {
                        return false;
                    }
                    self.external_grants
                        .grant_for_capabilities(
                            session,
                            workspace,
                            path,
                            operation.content.identity.turn_task_id,
                            operation.content.identity.correlation_id,
                            &[*capability],
                            enumerates,
                        )
                        .ok()
                        .flatten()
                        .is_some_and(|grant| {
                            let root = std::path::Path::new(&grant.root_display);
                            !super::super::protected_paths::escapes_recursive_grant(root, path)
                                && !self.control_resources.grant_escapes_into_data(root, path)
                        })
                }
                _ => false,
            });
            if verified {
                covered.push(*capability);
            }
        }
        covered
    }
}
