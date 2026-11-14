use super::*;
use aura_domain_common::{ApprovalRequestId, PermissionGrantId, PrincipalId, Revision};
use aura_security_contract::{
    AuthorizationLifetime, AuthorizationScope, BackendClass, BackendRequirement,
    OperationFingerprint, SecurityProfile,
};

const FOLDER: &str = r"\\?\C:\Fixture\Workspace";

fn approval(workspace_root: Option<&str>) -> AuthorizationGrant {
    let session_id = SessionId::new();
    let fingerprint =
        OperationFingerprint(aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"approved"));
    let mut scope = AuthorizationScope::exact(
        fingerprint.clone(),
        SecurityProfile::Guided,
        BackendRequirement {
            allowed_classes: [BackendClass::CoreBroker].into(),
            guarantees: Default::default(),
        },
        AuthorizationLifetime::Session { session_id },
    );
    scope.file_consent = Some(FileSessionConsentRef {
        approval_id: ApprovalRequestId::new(),
        session_id,
        principal_id: PrincipalId::new(),
        capabilities: vec![ResolvedCapability::WorkspaceFileWrite],
        targets: vec![],
        approved_operation_fingerprint: fingerprint,
        grant_revision: Revision::INITIAL,
        revoked_grant_revision: None,
        workspace_root: workspace_root.map(str::to_owned),
    });
    AuthorizationGrant {
        grant_id: PermissionGrantId::new(),
        revision: Revision::INITIAL,
        scope,
        issued_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    }
}

fn at(canonical_root: &str, epoch: u64) -> ApprovalWorkspace {
    ApprovalWorkspace {
        canonical_root: canonical_root.into(),
        authorization_epoch: Revision::new(epoch),
    }
}

#[test]
fn only_approvals_out_of_a_workspace_core_can_read_are_withdrawn() {
    let (restored, moved, fresh, unreadable) =
        (WorkspaceId::new(), WorkspaceId::new(), WorkspaceId::new(), WorkspaceId::new());
    let workspaces = BTreeMap::from([
        // Archived and restored: a later epoch at the same folder.
        (restored, at(FOLDER, 3)),
        (moved, at(r"\\?\D:\Moved\Workspace", 2)),
        (fresh, at(FOLDER, 1)),
    ]);
    let at_its_folder = approval(Some(FOLDER));
    let from_the_old_folder = approval(Some(FOLDER));
    let without_folder_past_epoch_one = approval(None);
    let without_folder_at_epoch_one = approval(None);
    let in_a_workspace_core_cannot_read = approval(Some(r"\\?\E:\Elsewhere"));
    let expected = [from_the_old_folder.grant_id, without_folder_past_epoch_one.grant_id];
    let withdrawn = out_of_workspace(
        vec![
            (at_its_folder, restored),
            (from_the_old_folder, moved),
            (without_folder_past_epoch_one, restored),
            (without_folder_at_epoch_one, fresh),
            (in_a_workspace_core_cannot_read, unreadable),
        ],
        &workspaces,
    );
    assert_eq!(withdrawn.iter().map(|(grant, _)| grant.grant_id).collect::<Vec<_>>(), expected);
    assert!(
        withdrawn.iter().all(|(grant, consent)| grant.scope.file_consent.as_ref() == Some(consent)),
        "each is withdrawn with the approval it carries"
    );
}
