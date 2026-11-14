//! A V2 package tool has no row in this store. Its credential target resolves
//! only through the facts Core supplies, and only at the installed revision.
use aura_domain_common::{
    Revision, RuntimeInstanceId, SessionId, ToolId, ToolInstanceId, ToolPackageId, WorkspaceId,
};
use aura_persistence::{
    PackageToolCredentialFacts, PersistenceCoordinator, PersistenceError, PersistenceOptions,
    PersistenceStartup, ToolCredentialScopeSource,
};
use aura_tool_contract::CredentialSlotKey;
use aura_tool_domain::{ToolCredentialManagementTargetV1, ToolCredentialPlacementV1};
use std::sync::Arc;

struct Facts(Option<PackageToolCredentialFacts>);
impl ToolCredentialScopeSource for Facts {
    fn session_belongs_to(
        &self,
        _workspace: WorkspaceId,
        _session: SessionId,
    ) -> Result<bool, PersistenceError> {
        Ok(false)
    }
    fn package_tool_target(
        &self,
        instance: ToolInstanceId,
    ) -> Result<Option<PackageToolCredentialFacts>, PersistenceError> {
        Ok(self.0.clone().filter(|_| instance == INSTANCE))
    }
}

const INSTANCE: ToolInstanceId = ToolInstanceId::from_bytes([7; 16]);

fn store(facts: Option<PackageToolCredentialFacts>) -> (tempfile::TempDir, PersistenceCoordinator) {
    let root = tempfile::tempdir().expect("temporary V1 store");
    let PersistenceStartup::Ready(persistence) =
        PersistenceCoordinator::start(PersistenceOptions::new(
            root.path().join("data/aura.db"),
            root.path().join("data/artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("persistence")
    else {
        panic!("fresh store needs no recovery")
    };
    persistence.bind_tool_credential_scope_owner(Arc::new(Facts(facts))).expect("scope owner");
    (root, persistence)
}

fn facts() -> PackageToolCredentialFacts {
    PackageToolCredentialFacts {
        tool_id: ToolId::from_bytes([8; 16]),
        package_id: ToolPackageId::new(),
        instance_revision: Revision::new(3),
        slot_key: CredentialSlotKey::new("api_key").expect("slot"),
        placement: ToolCredentialPlacementV1::HttpBearerAuthorization,
    }
}

fn target(revision: u64, placement: ToolCredentialPlacementV1) -> ToolCredentialManagementTargetV1 {
    ToolCredentialManagementTargetV1 {
        tool_instance_id: INSTANCE,
        expected_instance_revision: Revision::new(revision),
        credential_slot_key: CredentialSlotKey::new("api_key").expect("slot"),
        placement,
        workspace_id: None,
        session_id: None,
    }
}

#[test]
fn a_package_tool_target_resolves_only_through_core_facts_at_the_installed_revision() {
    let bearer = ToolCredentialPlacementV1::HttpBearerAuthorization;
    let (_root, without) = store(None);
    assert!(
        matches!(
            without.list_managed_tool_credentials_v1(target(3, bearer.clone())),
            Err(PersistenceError::ToolEntityNotFound)
        ),
        "no facts: an unknown instance is not a credential target"
    );
    let (_root, with) = store(Some(facts()));
    assert_eq!(
        with.list_managed_tool_credentials_v1(target(3, bearer.clone())).expect("listed"),
        vec![],
        "the installed package tool is a target with no credentials yet"
    );
    assert!(
        matches!(
            with.list_managed_tool_credentials_v1(target(2, bearer)),
            Err(PersistenceError::ToolEntityRevisionConflict)
        ),
        "a target from before a reinstall is refused"
    );
    let header = ToolCredentialPlacementV1::HttpFixedHeader {
        header_name: aura_tool_contract::HttpHeaderNameV1::new("x-api-key").expect("header"),
    };
    assert!(
        matches!(
            with.list_managed_tool_credentials_v1(target(3, header)),
            Err(PersistenceError::ToolEntityNotFound)
        ),
        "a placement the package did not declare is not this tool's slot"
    );
}
