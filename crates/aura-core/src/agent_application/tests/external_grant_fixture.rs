use super::*;

/// One authorized external directory, recorded the way the approval flow records it.
pub(super) fn external_directory_grant(
    session: SessionId,
    workspace_id: aura_domain_common::WorkspaceId,
    external: &std::path::Path,
    capabilities: Vec<aura_permission_domain::ExternalGrantCapability>,
) -> aura_permission_domain::ExternalGrantRecord {
    let identity = aura_windows_platform::inspect_existing_path(external, true)
        .expect("external directory identity");
    aura_permission_domain::ExternalGrantRecord {
        id: aura_domain_common::ExternalGrantId::new(),
        session_id: session,
        workspace_id,
        permission_grant_id: None,
        selected_root: aura_security_contract::CanonicalPath {
            utf16: external.display().to_string().encode_utf16().collect(),
            display: external.display().to_string(),
        },
        canonical_root: aura_security_contract::CanonicalPath {
            utf16: identity.final_path.display().to_string().encode_utf16().collect(),
            display: identity.final_path.display().to_string(),
        },
        root_identity: aura_workspace::domain_identity(&identity.identity),
        target_kind: aura_permission_domain::ExternalGrantTargetKind::Directory,
        capabilities,
        recursive_scope: true,
        lifecycle: aura_permission_domain::ExternalGrantLifecycle::Active,
        revalidation_revision: 1,
        created_at: aura_domain_common::UtcTimestamp::now(),
        updated_at: aura_domain_common::UtcTimestamp::now(),
        suspended_at: None,
        revoked_at: None,
        expires_at: None,
        turn_task_id: None,
        term: aura_permission_domain::ExternalGrantTerm::Session,
    }
}
