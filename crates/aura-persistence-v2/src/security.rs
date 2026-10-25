use aura_domain_common::{
    ApprovalRequestId, PermissionGrantId, Revision, RuntimeInstanceId, SessionId, UtcTimestamp,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_security_contract::{
    AuthorizationApproval, AuthorizationBasis, AuthorizationDecision, AuthorizationGrant,
    AuthorizationLifetime, NativeSessionConsentRef, OperationFingerprint,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// The one reusable authorization fact in the Store.
///
/// The Grant row carries the authority and the exact approval scope; this row
/// carries what makes that Grant reusable, and nothing else. Splitting them keeps
/// the Grant readable by every existing code path while making the reuse
/// conditions — Core runtime instance, session permission revision, approved
/// principal and Shell — explicit, queryable and revocable.
///
/// There is no permanent consent: the row is scoped to one Core runtime instance,
/// so a restarted Core leaves it inert rather than letting it be inherited.
#[derive(Clone, Debug, serde::Deserialize, Eq, PartialEq, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSessionConsent {
    #[serde(default)]
    pub lifetime: aura_security_contract::ShellConsentLifetime,
    pub grant_id: PermissionGrantId,
    pub revision: Revision,
    pub approval_id: ApprovalRequestId,
    pub session_id: SessionId,
    pub runtime_instance: RuntimeInstanceId,
    pub permission_revision: u64,
    pub approved: aura_security_contract::NativeSessionConsentScope,
    /// The Operation the user confirmed, kept for the management surface and for
    /// audit. A later call is not required to match it.
    pub approved_operation_fingerprint: OperationFingerprint,
    /// The Grant revision a withdrawal produced, when one has happened.
    ///
    /// Recorded here rather than inferred so the management projection can tell a
    /// withdrawn consent from a row that no longer describes its Grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_grant_revision: Option<Revision>,
}

impl NativeSessionConsent {
    #[must_use]
    pub fn reference(&self) -> NativeSessionConsentRef {
        NativeSessionConsentRef {
            lifetime: self.lifetime,
            grant_id: self.grant_id,
            grant_revision: self.revision,
            revoked_grant_revision: self.revoked_grant_revision,
            approval_id: self.approval_id,
            core_runtime_instance_id: self.runtime_instance,
            session_permission_revision: self.permission_revision,
            approved: self.approved.clone(),
            approved_operation_fingerprint: self.approved_operation_fingerprint.clone(),
        }
    }

    /// Whether the stored Grant still describes this consent.
    ///
    /// Exactly two revisions may legitimately differ from the issued one: the revision
    /// of a live Grant (the issued revision) and the revision a withdrawal recorded.
    /// Everything else is an immutable binding, and a row and a Grant that disagree
    /// about any of it are two different authorizations — the management surface would
    /// otherwise show a consent as usable whose principal, runtime, permission revision,
    /// Shell range or approved Operation had been replaced, while the Store's own
    /// reservation refused the same pair. A binding that is only checked in the
    /// reservation is a binding the person reading the list cannot see.
    #[must_use]
    pub fn matches_grant(&self, grant: &AuthorizationGrant) -> bool {
        if grant.grant_id != self.grant_id {
            return false;
        }
        let Some(reference) = grant.scope.consent.as_ref() else {
            return false;
        };
        // A consent Grant is a session lifetime over exactly this consent's session.
        // Anything else cannot be the Grant this row was issued with.
        if !matches!(
            grant.scope.lifetime,
            AuthorizationLifetime::Session { session_id } if session_id == self.session_id
        ) {
            return false;
        }
        reference.grant_id == self.grant_id
            && reference.lifetime == self.lifetime
            && reference.grant_revision == self.revision
            && reference.accepts_grant_revision(grant.revision)
            && reference.revoked_grant_revision == self.revoked_grant_revision
            && reference.approval_id == self.approval_id
            && reference.core_runtime_instance_id == self.runtime_instance
            && reference.session_permission_revision == self.permission_revision
            && reference.approved == self.approved
            && reference.approved_operation_fingerprint == self.approved_operation_fingerprint
    }
}

/// Records the consent that makes one already-inserted Grant reusable.
///
/// The caller has already validated that the approved Operation may outlive its
/// own fingerprint. This only refuses to record the same approval twice, so a
/// replayed approval command cannot re-issue a consent after revocation, and
/// refuses a consent whose Grant does not carry the matching reference.
pub fn insert_native_session_consent(
    tx: &Transaction<'_>,
    consent: &NativeSessionConsent,
) -> StoreResult<()> {
    if consent.revision.value() == 0 || consent.permission_revision == 0 {
        return Err(Error::Invalid("native session consent revision"));
    }
    let existing: Option<String> = tx
        .query_row(
            "SELECT grant_id FROM native_session_consents WHERE approval_id=?1",
            [consent.approval_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Err(Error::Conflict);
    }
    let grant = grant(tx, consent.grant_id)?.ok_or(Error::Conflict)?;
    if grant.revision != consent.revision
        || grant.scope.consent.as_ref() != Some(&consent.reference())
    {
        return Err(Error::Conflict);
    }
    tx.execute(
        "INSERT INTO native_session_consents VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",
        params![
            consent.grant_id.to_string(),
            consent.approval_id.to_string(),
            consent.session_id.to_string(),
            consent.runtime_instance.to_string(),
            record::sql_u64(consent.permission_revision)?,
            consent.approved.approved_shell.storage_key(),
            record::sql_u64(consent.revision.value())?,
            record::encode(consent)?,
        ],
    )?;
    audit(
        tx,
        &consent.grant_id.to_string(),
        "native_session_consent.created",
        consent,
        UtcTimestamp::now(),
    )
}

/// Consents recorded for one session. A revoked or consumed Grant stays visible
/// here so the management surface can explain it; liveness is the Grant's.
pub fn native_session_consents(
    connection: &Connection,
    session: SessionId,
) -> StoreResult<Vec<NativeSessionConsent>> {
    let mut query = connection.prepare(
        "SELECT payload FROM native_session_consents WHERE session_id=?1 ORDER BY grant_id",
    )?;
    query
        .query_map([session.to_string()], |row| row.get::<_, String>(0))?
        .map(|value| record::decode(&value?))
        .collect()
}

pub fn native_session_consent(
    connection: &Connection,
    grant_id: PermissionGrantId,
) -> StoreResult<Option<NativeSessionConsent>> {
    let payload: Option<String> = connection
        .query_row(
            "SELECT payload FROM native_session_consents WHERE grant_id=?1",
            [grant_id.to_string()],
            |row| row.get(0),
        )
        .optional()?;
    payload.map(|value| record::decode(&value)).transpose()
}

pub fn insert_grant(tx: &Transaction<'_>, grant: &AuthorizationGrant) -> StoreResult<()> {
    if grant.revision.value() == 0 {
        return Err(Error::Invalid("grant revision"));
    }
    tx.execute(
        "INSERT INTO authorization_grants VALUES(?1,?2,?3,?4,?5,?6)",
        params![
            grant.grant_id.to_string(),
            record::sql_u64(grant.revision.value())?,
            record::encode(&grant.scope.operation_fingerprint)?,
            grant.revoked_at.is_some(),
            grant.consumed_at.is_some(),
            record::encode(grant)?
        ],
    )?;
    audit(tx, &grant.grant_id.to_string(), "grant.created", grant, grant.issued_at)
}
pub fn grant(
    connection: &Connection,
    id: PermissionGrantId,
) -> StoreResult<Option<AuthorizationGrant>> {
    let v: Option<String> = connection
        .query_row("SELECT payload FROM authorization_grants WHERE id=?1", [id.to_string()], |r| {
            r.get(0)
        })
        .optional()?;
    v.map(|v| record::decode(&v)).transpose()
}
/// A snapshot predates its approval: reload exact current grants from this Store.
pub fn grants_for_operation(
    connection: &Connection,
    fingerprint: &aura_security_contract::OperationFingerprint,
) -> StoreResult<Vec<AuthorizationGrant>> {
    let mut query = connection
        .prepare("SELECT payload FROM authorization_grants WHERE operation_hash=?1 ORDER BY id")?;
    query
        .query_map([record::encode(fingerprint)?], |r| r.get::<_, String>(0))?
        .map(|v| record::decode(&v?))
        .collect()
}

/// Every Grant that could authorize `fingerprint` for this session.
///
/// Two disjoint sets, in a fixed order so the answer is deterministic:
///
/// 1. Grants stored for exactly this Operation fingerprint — every historical and
///    exact-operation authorization, unchanged.
/// 2. Grants for this session that carry a reusable Native session consent. These
///    are stored under the fingerprint of the Operation the user approved, so a
///    fingerprint-only query would never find them, and a new call would ask again
///    for a permission the user already gave. Handing them to policy is safe
///    because policy re-checks the consent binding, the Core runtime instance, the
///    live session permission revision, the principal and the Shell. An MCP
///    turn answer is found the same way; policy binds it to its turn and server.
///
/// A consent Grant for another session is never returned: consent reuse is bounded
/// by the session the user approved, not by the installation.
pub fn grants_for_call(
    connection: &Connection,
    fingerprint: &aura_security_contract::OperationFingerprint,
    session: Option<SessionId>,
) -> StoreResult<Vec<AuthorizationGrant>> {
    let mut grants = grants_for_operation(connection, fingerprint)?;
    let Some(session) = session else {
        return Ok(grants);
    };
    if crate::workspace::session(connection, session)?.is_none() {
        grants.retain(|grant| grant.scope.is_exact_operation());
        return Ok(grants);
    }
    let mut query = connection.prepare(
        "SELECT payload FROM authorization_grants WHERE id IN \
         (SELECT grant_id FROM native_session_consents WHERE session_id=?1) \
         OR json_extract(payload,'$.scope.file_consent.session_id')=?1 \
         OR json_extract(payload,'$.scope.memory_consent.session_id')=?1 \
         OR json_extract(payload,'$.scope.network_consent.session_id')=?1 \
         OR json_extract(payload,'$.scope.mcp_consent.session_id')=?1 ORDER BY id",
    )?;
    let mut seen =
        grants.iter().map(|grant| grant.grant_id).collect::<std::collections::BTreeSet<_>>();
    for payload in query.query_map([session.to_string()], |r| r.get::<_, String>(0))? {
        let grant: AuthorizationGrant = record::decode(&payload?)?;
        // A consent Grant is only reusable while the reference it carries is the
        // one policy will validate. Anything else is a corrupt row, and dropping
        // it fails closed.
        if grant.scope.is_exact_operation() || !seen.insert(grant.grant_id) {
            continue;
        }
        grants.push(grant);
    }
    Ok(grants)
}

/// Revokes a Grant, or, when `expected_consent` is given, only a Grant that still
/// carries exactly that reusable consent.
///
/// The second form is what the Native session management surface uses. Withdrawing
/// a consent has to be a compare-and-swap on the consent identity, not on the Grant
/// id alone: a Grant id that has since been reissued, or a Grant whose consent
/// reference moved, is a different authorization and must not be revoked by a stale
/// management action.
pub fn revoke(
    tx: &Transaction<'_>,
    id: PermissionGrantId,
    expected: Revision,
    expected_consent: Option<&NativeSessionConsentRef>,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let mut g = grant(tx, id)?.ok_or(Error::Conflict)?;
    if g.revision != expected || g.revoked_at.is_some() {
        return Err(Error::Conflict);
    }
    if expected_consent.is_some_and(|consent| g.scope.consent.as_ref() != Some(consent)) {
        return Err(Error::Conflict);
    }
    g.revision = Revision::new(expected.value().checked_add(1).ok_or(Error::Conflict)?);
    g.revoked_at = Some(now);
    // A withdrawal moves the Grant's revision, so the consent reference inside the
    // Grant has to record the revision it moved to. Without this the reference
    // describes a revision the Grant no longer holds, and every reader that checks
    // the reference would read a successful withdrawal as a corrupt row — which is
    // exactly how a committed revocation came back to the UI as a failure.
    if let Some(reference) = g.scope.file_consent.as_mut() {
        reference.revoked_grant_revision = Some(g.revision);
    }
    if let Some(reference) = g.scope.memory_consent.as_mut() {
        reference.revoked_grant_revision = Some(g.revision);
    }
    if let Some(reference) = g.scope.consent.as_mut() {
        reference.revoked_grant_revision = Some(g.revision);
    }
    if let Some(reference) = g.scope.network_consent.as_mut() {
        reference.revoked_grant_revision = Some(g.revision);
    }
    record::changed(tx.execute("UPDATE authorization_grants SET revision=?2,revoked=1,payload=?3 WHERE id=?1 AND revision=?4",params![id.to_string(),record::sql_u64(g.revision.value())?,record::encode(&g)?,record::sql_u64(expected.value())?])?)?;
    // The consent row is the management surface's own record, so it carries the same
    // fact. It is written in this transaction: a row that disagrees with its Grant
    // would make the surface report a state the dispatch path never reaches.
    if let Some(mut stored) = native_session_consent(tx, id)? {
        stored.revoked_grant_revision = Some(g.revision);
        record::changed(tx.execute(
            "UPDATE native_session_consents SET payload=?2 WHERE grant_id=?1",
            params![id.to_string(), record::encode(&stored)?],
        )?)?;
    }
    audit(tx, &id.to_string(), "grant.revoked", &g, now)
}

/// Same transaction as Attempt reservation; the caller also validates current
/// policy, source, file and credential facts before reaching this store.
/// Whether a stored Grant's scope and a decision's scope describe the same
/// authorization.
///
/// For an exact-operation Grant the two must be equal, which is what makes a
/// decision for one Operation impossible to replay for another. For a reusable
/// file or Native session consent this reservation is already holding the Grant itself
/// rather than a copy of it, so there is nothing for the decision to substitute:
/// what has to hold is that the decision was derived from *this* consent for *this*
/// Operation and did not rewrite any of the consent's own terms. Treating that as
/// scope inequality would refuse every legitimate reuse.
pub(crate) fn scope_authorizes(
    stored: &aura_security_contract::AuthorizationScope,
    decision: &aura_security_contract::AuthorizationScope,
) -> bool {
    aura_security_contract::AuthorizationScope::derive_for(stored, &decision.operation_fingerprint)
        .as_ref()
        == Some(decision)
}

pub(crate) fn reserve_authority(
    tx: &Transaction<'_>,
    decision: &AuthorizationDecision,
    operation: &aura_execution_contract::SecuredOperation,
    live_permission_revision: u64,
    now: UtcTimestamp,
) -> StoreResult<()> {
    if now < decision.decided_at || now >= decision.expires_at {
        return Err(Error::Conflict);
    }
    if matches!(decision.basis, AuthorizationBasis::Composite { .. }) {
        return crate::network_consents::reserve(
            tx,
            decision,
            operation,
            live_permission_revision,
            now,
        );
    }
    if operation
        .content
        .capabilities
        .contains(&aura_security_contract::ResolvedCapability::OrdinaryNetwork)
    {
        return Err(Error::Conflict);
    }
    let AuthorizationBasis::Grant { .. } = decision.basis else {
        if let AuthorizationBasis::AuthenticatedMain { .. } = decision.basis {
            if !matches!(decision.scope.lifetime, AuthorizationLifetime::Once { .. })
                || decision.scope.backend_requirement.allowed_classes
                    != [aura_security_contract::BackendClass::CoreBroker].into()
            {
                return Err(Error::Conflict);
            }
            // Channel liveness is checked by Core immediately before this
            // transaction; the Attempt's unique operation key consumes Once.
            return audit(tx, "authenticated_main", "ui.dispatch_reserved", decision, now);
        }
        if let AuthorizationBasis::PermissionMode {
            snapshot_id,
            mode,
            session_permission_revision,
        } = decision.basis
        {
            let snapshot = snapshot(tx, snapshot_id)?.ok_or(Error::Conflict)?;
            validate_snapshot_binding(tx, &snapshot)?;
            if snapshot.mode != mode
                || snapshot.session_permission_revision != session_permission_revision
            {
                return Err(Error::Conflict);
            }
        }
        return Ok(());
    };
    reserve_grant(tx, decision, live_permission_revision, now)
}

/// Called only after complete operation/slice coverage has been verified.
pub(crate) fn reserve_grant(
    tx: &Transaction<'_>,
    decision: &AuthorizationDecision,
    live_permission_revision: u64,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let AuthorizationBasis::Grant { grant_id, revision } = decision.basis else {
        return Err(Error::Conflict);
    };
    let mut current = grant(tx, grant_id)?.ok_or(Error::Conflict)?;
    if current.revision != revision
        || !scope_authorizes(&current.scope, &decision.scope)
        || !current.is_live_at(now)
        || decision.expires_at <= now
        || current.expires_at.is_some_and(|limit| decision.expires_at > limit)
    {
        return Err(Error::Conflict);
    }
    if let Some(consent) = current.scope.consent.clone() {
        reserve_native_session_consent(tx, &consent, &current, live_permission_revision, now)?;
    }
    if matches!(current.scope.lifetime, AuthorizationLifetime::Once { .. }) {
        current.consumed_at = Some(now);
        record::changed(tx.execute("UPDATE authorization_grants SET consumed=1,payload=?2 WHERE id=?1 AND revision=?3 AND revoked=0 AND consumed=0",params![grant_id.to_string(),record::encode(&current)?,record::sql_u64(revision.value())?])?)?;
        audit(tx, &grant_id.to_string(), "grant.consumed", &current, now)?;
    }
    Ok(())
}

/// Re-checks a reusable consent immediately before the Attempt is reserved.
///
/// [`crate::security::grants_for_call`] already selected this Grant for the call,
/// and policy already matched the consent binding. Repeating the check inside the
/// owning transaction is what makes revocation, a permission change and a Core
/// restart effective against a decision that was derived a moment earlier: the
/// facts are read here, in the same transaction that creates the Attempt, so there
/// is no window in which a withdrawn consent still reserves work.
///
/// `live_permission_revision` is the session's current permission revision, read
/// by Core under the same mode barrier that is held across this transaction. It is
/// passed in rather than re-read here so that "the live authority" has exactly one
/// owner instead of a second copy that could disagree.
fn reserve_native_session_consent(
    tx: &Transaction<'_>,
    consent: &NativeSessionConsentRef,
    grant: &AuthorizationGrant,
    live_permission_revision: u64,
    now: UtcTimestamp,
) -> StoreResult<()> {
    let stored = native_session_consent(tx, consent.grant_id)?.ok_or(Error::Conflict)?;
    // The stored Grant carries this consent, so the reference has to describe the
    // Grant it came from and nothing may have moved since it was issued. The Grant's
    // own live/revoked state was already checked by [`reserve_authority`], so this
    // only has to reject a row and a Grant that disagree.
    if stored.reference() != *consent || !stored.matches_grant(grant) {
        return Err(Error::Conflict);
    }
    // A revision newer than the one the user approved under means the session's
    // permissions changed and this consent does not carry over, including a
    // downgrade followed by a return to the same mode.
    if consent.lifetime == aura_security_contract::ShellConsentLifetime::LegacyRuntime
        && live_permission_revision != consent.session_permission_revision
    {
        return Err(Error::Conflict);
    }
    audit(tx, &consent.grant_id.to_string(), "native_session_consent.reserved", consent, now)
}

pub(crate) fn audit(
    tx: &Transaction<'_>,
    owner: &str,
    kind: &str,
    payload: &impl serde::Serialize,
    at: UtcTimestamp,
) -> StoreResult<()> {
    aura_persistence_core::event::append(
        tx,
        &aura_persistence_core::event::EventDraft {
            id: aura_domain_common::EventId::new(),
            owner,
            kind,
            version: 1,
            payload: &serde_json::to_value(payload)?,
            at,
        },
        "security",
    )?;
    Ok(())
}

pub fn insert_approval(tx: &Transaction<'_>, approval: &AuthorizationApproval) -> StoreResult<()> {
    if approval.revision.value() != 1
        || approval.state != aura_security_contract::ApprovalState::Pending
        || approval.expires_at <= approval.requested_at
        || approval.allowed_decisions.is_empty()
    {
        return Err(Error::Invalid("approval proposal"));
    }
    tx.execute(
        "INSERT INTO approval_requests VALUES(?1,?2,?3,?4)",
        params![
            approval.approval_id.to_string(),
            record::sql_u64(approval.revision.value())?,
            record::encode(&approval.state)?,
            record::encode(approval)?
        ],
    )?;
    audit(
        tx,
        &approval.approval_id.to_string(),
        "approval.created",
        approval,
        approval.requested_at,
    )
}

pub fn insert_snapshot(
    tx: &Transaction<'_>,
    snapshot: &aura_security_contract::PermissionSnapshot,
) -> StoreResult<()> {
    if snapshot.session_permission_revision == 0 {
        return Err(Error::Invalid("permission revision"));
    }
    let workspace: String = tx.query_row(
        "SELECT workspace_id FROM sessions WHERE id=?1",
        [snapshot.session_id.to_string()],
        |r| r.get(0),
    )?;
    if workspace != snapshot.workspace_id.to_string() {
        return Err(Error::Conflict);
    }
    let mut query = tx.prepare("SELECT payload FROM permission_snapshots WHERE session_id=?1")?;
    for value in query.query_map([snapshot.session_id.to_string()], |r| r.get::<_, String>(0))? {
        let current: aura_security_contract::PermissionSnapshot = record::decode(&value?)?;
        if current.session_permission_revision > snapshot.session_permission_revision
            || current.session_permission_revision == snapshot.session_permission_revision
                && current.mode != snapshot.mode
        {
            return Err(Error::Conflict);
        }
    }
    tx.execute(
        "INSERT INTO permission_snapshots VALUES(?1,?2,?3)",
        params![
            snapshot.id.to_string(),
            snapshot.session_id.to_string(),
            record::encode(snapshot)?
        ],
    )?;
    Ok(())
}

pub fn snapshot(
    connection: &Connection,
    id: aura_domain_common::PermissionSnapshotId,
) -> StoreResult<Option<aura_security_contract::PermissionSnapshot>> {
    let value: Option<String> = connection
        .query_row("SELECT payload FROM permission_snapshots WHERE id=?1", [id.to_string()], |r| {
            r.get(0)
        })
        .optional()?;
    value.map(|v| record::decode(&v)).transpose()
}

/// Validates journal integrity, not the live session mode. Core also holds the
/// shared mode-authority barrier while reserving a dispatch; a historical
/// snapshot (including the newest one here) never grants current permission.
pub fn validate_snapshot_binding(
    connection: &Connection,
    expected: &aura_security_contract::PermissionSnapshot,
) -> StoreResult<()> {
    validate_recorded_snapshot(connection, expected)?;
    let mut query =
        connection.prepare("SELECT payload FROM permission_snapshots WHERE session_id=?1")?;
    for value in query.query_map([expected.session_id.to_string()], |r| r.get::<_, String>(0))? {
        let current: aura_security_contract::PermissionSnapshot = record::decode(&value?)?;
        if current.session_permission_revision > expected.session_permission_revision {
            return Err(Error::Conflict);
        }
    }
    Ok(())
}

/// Checks the immutable snapshot identity. The caller must hold and verify
/// current session authority separately when retaining a prepared operation.
pub fn validate_recorded_snapshot(
    connection: &Connection,
    expected: &aura_security_contract::PermissionSnapshot,
) -> StoreResult<()> {
    if snapshot(connection, expected.id)?.as_ref() != Some(expected) {
        return Err(Error::Conflict);
    }
    Ok(())
}
