use aura_domain_common::{
    ExternalGrantId, FileIdentity, PermissionGrantId, SessionId, TurnTaskId, UtcTimestamp,
    WorkspaceId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use aura_security_contract::CanonicalPath;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExternalGrantCapability {
    Read,
    Write,
    Create,
    Delete,
    ModelEgress,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalGrantTargetKind {
    File,
    Directory,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalGrantLifecycle {
    Active,
    Suspended,
    Revoked,
}

/// How long a directory grant remains reusable after the user approves it.
///
/// This is the grant's own term, not the approval request's confirmation window.
/// Older records omit the field and stay session-scoped, which is the term they had.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalGrantTerm {
    #[default]
    Session,
    Workspace,
    Turn,
}

/// Core-owned capability record. Paths and file identities are verification
/// facts; only `ExternalGrantId` is an authority-bearing handle.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalGrantRecord {
    pub id: ExternalGrantId,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub permission_grant_id: Option<PermissionGrantId>,
    pub selected_root: CanonicalPath,
    pub canonical_root: CanonicalPath,
    pub root_identity: FileIdentity,
    pub target_kind: ExternalGrantTargetKind,
    pub capabilities: Vec<ExternalGrantCapability>,
    pub recursive_scope: bool,
    pub lifecycle: ExternalGrantLifecycle,
    pub revalidation_revision: u64,
    pub created_at: UtcTimestamp,
    pub updated_at: UtcTimestamp,
    pub suspended_at: Option<UtcTimestamp>,
    pub revoked_at: Option<UtcTimestamp>,
    /// A clock bound on top of [`Self::term`]. Absent means the term itself is the bound:
    /// the session, the workspace, or the turn, plus revocation. It is not the approval
    /// request's confirmation deadline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<UtcTimestamp>,
    /// Set only when [`Self::term`] is [`ExternalGrantTerm::Turn`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_task_id: Option<TurnTaskId>,
    #[serde(default)]
    pub term: ExternalGrantTerm,
}

/// Renderer/model-safe view. It intentionally omits the canonical path and
/// file identity so callers cannot reflect either value back as authority.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalGrantView {
    pub id: ExternalGrantId,
    pub session_id: SessionId,
    pub workspace_id: WorkspaceId,
    pub display_path: String,
    pub target_kind: ExternalGrantTargetKind,
    pub capabilities: Vec<ExternalGrantCapability>,
    pub recursive_scope: bool,
    pub lifecycle: ExternalGrantLifecycle,
    pub revalidation_revision: u64,
    pub expires_at: Option<UtcTimestamp>,
    pub term: ExternalGrantTerm,
    /// Whether this grant can authorize a call right now. Lifecycle alone is not enough:
    /// an active row can still be past its clock.
    #[serde(default)]
    pub availability: ExternalGrantAvailability,
}

/// The projected force of a directory or file grant. Turn end is decided by the caller
/// that knows the session's current turn; this value is what the management UI shows.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalGrantAvailability {
    #[default]
    InForce,
    Expired,
    Suspended,
    Revoked,
    /// The grant was limited to one turn, and that turn is no longer the session's
    /// current turn. The row is not revoked; it simply cannot authorize another call.
    TurnEnded,
}

impl From<&ExternalGrantRecord> for ExternalGrantView {
    fn from(value: &ExternalGrantRecord) -> Self {
        Self {
            id: value.id,
            session_id: value.session_id,
            workspace_id: value.workspace_id,
            display_path: value.selected_root.display.clone(),
            target_kind: value.target_kind,
            capabilities: value.capabilities.clone(),
            recursive_scope: value.recursive_scope,
            lifecycle: value.lifecycle,
            revalidation_revision: value.revalidation_revision,
            expires_at: value.expires_at,
            term: value.term,
            availability: grant_availability(value, UtcTimestamp::now()),
        }
    }
}

/// Clock and lifecycle together. An active row past `expires_at` is not in force.
pub fn grant_availability(
    record: &ExternalGrantRecord,
    now: UtcTimestamp,
) -> ExternalGrantAvailability {
    if record.expires_at.is_some_and(|expiry| expiry <= now) {
        return ExternalGrantAvailability::Expired;
    }
    match record.lifecycle {
        ExternalGrantLifecycle::Active => ExternalGrantAvailability::InForce,
        ExternalGrantLifecycle::Suspended => ExternalGrantAvailability::Suspended,
        ExternalGrantLifecycle::Revoked => ExternalGrantAvailability::Revoked,
    }
}

/// Applies turn scope after the clock and lifecycle decision.
///
/// An active turn grant stays in force only while `turn_open` is true. Session and
/// workspace grants ignore that flag. Expiry, suspension and revocation still win.
pub fn project_grant_availability(
    record: &ExternalGrantRecord,
    now: UtcTimestamp,
    turn_open: bool,
) -> ExternalGrantAvailability {
    let availability = grant_availability(record, now);
    if availability == ExternalGrantAvailability::InForce
        && record.term == ExternalGrantTerm::Turn
        && !turn_open
    {
        ExternalGrantAvailability::TurnEnded
    } else {
        availability
    }
}

/// Main-only input produced from a native file picker. This command is never
/// exposed through `window.aura.v1` as a generic path-taking method.
#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AttachExternalReadOnlyInput {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub selected_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeriveWorkspaceExternalGrantInput {
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
    pub permission_grant_id: PermissionGrantId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ListSessionExternalGrantsInput {
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevalidateExternalGrantInput {
    pub external_grant_id: ExternalGrantId,
    pub session_id: SessionId,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RevokeExternalGrantInput {
    pub external_grant_id: ExternalGrantId,
    pub session_id: SessionId,
}

#[cfg(test)]
mod tests {
    use super::{
        ExternalGrantAvailability, ExternalGrantCapability, ExternalGrantLifecycle,
        ExternalGrantRecord, ExternalGrantTargetKind, ExternalGrantTerm,
        project_grant_availability,
    };
    use aura_domain_common::{
        ExternalGrantId, FileIdentity, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId,
    };
    use aura_security_contract::CanonicalPath;

    fn turn_grant(expires_at: Option<UtcTimestamp>) -> ExternalGrantRecord {
        let now = UtcTimestamp::now();
        let path = "D:\\notes";
        ExternalGrantRecord {
            id: ExternalGrantId::new(),
            session_id: SessionId::new(),
            workspace_id: WorkspaceId::new(),
            permission_grant_id: None,
            selected_root: CanonicalPath {
                display: path.to_owned(),
                utf16: path.encode_utf16().collect(),
            },
            canonical_root: CanonicalPath {
                display: path.to_owned(),
                utf16: path.encode_utf16().collect(),
            },
            root_identity: FileIdentity {
                volume_serial_number: "1".to_owned(),
                file_id: "00000000000000000000000000000001".to_owned(),
            },
            target_kind: ExternalGrantTargetKind::Directory,
            capabilities: vec![ExternalGrantCapability::Read],
            recursive_scope: true,
            lifecycle: ExternalGrantLifecycle::Active,
            revalidation_revision: 1,
            created_at: now,
            updated_at: now,
            suspended_at: None,
            revoked_at: None,
            expires_at,
            turn_task_id: Some(TurnTaskId::new()),
            term: ExternalGrantTerm::Turn,
        }
    }

    #[test]
    fn a_finished_turn_grant_is_not_in_force() {
        let now = UtcTimestamp::now();
        let record = turn_grant(None);
        assert_eq!(
            project_grant_availability(&record, now, true),
            ExternalGrantAvailability::InForce
        );
        assert_eq!(
            project_grant_availability(&record, now, false),
            ExternalGrantAvailability::TurnEnded
        );
        let mut expired = record;
        expired.expires_at = Some(now);
        assert_eq!(
            project_grant_availability(&expired, now, false),
            ExternalGrantAvailability::Expired
        );
        expired.lifecycle = ExternalGrantLifecycle::Revoked;
        expired.expires_at = None;
        assert_eq!(
            project_grant_availability(&expired, now, false),
            ExternalGrantAvailability::Revoked
        );
    }
}
