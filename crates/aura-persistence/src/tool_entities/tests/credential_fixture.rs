use super::*;
pub(super) fn credential_record(
    seed: u8,
    owner: PrincipalId,
    scope: ToolCredentialScopeV1,
    state: ToolCredentialStateV1,
    placement: ToolCredentialPlacementV1,
    allowed_tool_id: ToolId,
    display_name: &str,
) -> ToolCredentialAuthorityRecordV1 {
    ToolCredentialAuthorityRecordV1 {
        allowed_origins: Vec::new(),
        credential_id: CredentialId::from_bytes([seed; 16]),
        credential_revision: CredentialRevision::INITIAL,
        authenticated_owner: owner,
        scope,
        credential_kind: ToolCredentialKindV1::ApiKey,
        state,
        placement_policy: ToolCredentialPlacementPolicyV1 { allowed_placements: vec![placement] },
        usage_policy: ToolCredentialUsagePolicyV1::Explicit {
            tool_package_ids: Vec::new(),
            tool_ids: vec![allowed_tool_id],
        },
        display_name: display_name.to_owned(),
        row_revision: Revision::INITIAL,
        created_at: timestamp(),
        updated_at: timestamp(),
    }
}
