//! The Core-side reader for reusable Native session consent.
//!
//! The contract types live in `aura-permission-domain`, beside the other security
//! projections, because they are IPC payloads. This module is what turns the
//! Store's own facts — plus the two facts only Core holds, its runtime instance and
//! the session's live permission state — into them.
//!
//! It is deliberately a *view* and nothing more. The Grant supplies liveness, the
//! consent row supplies the binding, and the projection re-derives nothing that
//! dispatch would decide differently: a consent shown as in force here is one the
//! final dispatch guard would also accept, and one shown as revoked is one the same
//! guard refuses. Runtime and permission revision expire only legacy records;
//! new session answers follow D2, without reviving already invalid legacy answers.

use aura_domain_common::{SessionId, UtcTimestamp};
use aura_permission_domain::{
    ListNativeSessionConsentsInput, NativeSessionConsentAvailability,
    NativeSessionConsentScopeView, NativeSessionConsentView, RevokeNativeSessionConsentInput,
};
use aura_persistence_v2::security::NativeSessionConsent;
use aura_security_contract::AuthorizationGrant;

/// Why a consent management call failed, in the terms the IPC surface reports.
///
/// The variants are deliberately few and stable. `NotFound` covers both "no such
/// consent" and "that consent belongs to another session", so a caller cannot use
/// this route to probe which session holds a grant.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum NativeSessionConsentError {
    #[error("permission.session_not_found")]
    SessionNotFound,
    #[error("permission.grant_not_found")]
    NotFound,
    #[error("permission.grant_state_changed")]
    RevisionConflict,
    #[error("permission.grant_not_applicable")]
    Unusable,
    #[error("agent.journal_unbound")]
    JournalUnbound,
    #[error("permission.store_unavailable")]
    Store,
}

impl From<aura_persistence_core::PersistenceCoreError> for NativeSessionConsentError {
    fn from(error: aura_persistence_core::PersistenceCoreError) -> Self {
        use aura_persistence_core::PersistenceCoreError as E;
        match error {
            E::Conflict => Self::RevisionConflict,
            E::Invalid(_) => Self::NotFound,
            _ => Self::Store,
        }
    }
}

/// The live facts a consent's usability depends on.
///
/// Both come from Core, not from the consent row: the runtime instance is this
/// process, and the permission revision is the session's current authority. Passing
/// them in is what lets one projection function serve the list, the withdrawal
/// response and the tests without any of them inventing a value.
#[derive(Clone, Copy, Debug)]
pub struct LiveConsentFacts {
    /// This Core process, as Core publishes it to every other authorization check.
    pub core_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    /// The session's current permission revision, read under the mode barrier.
    pub session_permission_revision: u64,
}

/// Reads the consents of one session from the Store that dispatch also reads.
///
/// A consent with no readable Grant, or whose Grant no longer matches its row, is
/// skipped instead of being shown as usable. That is the fail-closed direction: a
/// management list that omits an authorization is safe, one that invents one is not.
pub fn list(
    journal: &crate::broker::BrokerJournal,
    permission_authority: &aura_persistence::PersistenceCoordinator,
    runtime: aura_domain_common::RuntimeInstanceId,
    input: &ListNativeSessionConsentsInput,
) -> Result<Vec<NativeSessionConsentView>, NativeSessionConsentError> {
    let now = UtcTimestamp::now();
    let live = live_facts(permission_authority, input.session_id, runtime)?;
    let store = journal.lock().map_err(|_| NativeSessionConsentError::Store)?;
    let database = store.database.connection();
    let workspace = aura_persistence_v2::workspace::session(database, input.session_id)?
        .ok_or(NativeSessionConsentError::SessionNotFound)?
        .workspace_id;
    let mut views = Vec::new();
    for consent in
        aura_persistence_v2::security::native_session_consents(database, input.session_id)?
    {
        let Some(grant) = aura_persistence_v2::security::grant(database, consent.grant_id)? else {
            continue;
        };
        let approval_expires_at =
            match aura_persistence_v2::security_approval::load(database, consent.approval_id) {
                Ok(approval) => approval.expires_at,
                // The approval row is the confirmation window, not the authority. A
                // consent whose approval row cannot be read still has a live Grant,
                // so it is shown against the instant it was issued rather than hidden.
                Err(_) => grant.issued_at,
            };
        let Some(view) = view(&consent, &grant, workspace, approval_expires_at, live, now) else {
            continue;
        };
        if !input.include_inactive && !view.availability.is_in_force() {
            continue;
        }
        views.push(view);
    }
    // Newest first: the consent a person is looking for is the one they just made.
    views.sort_by(|a, b| b.issued_at.cmp(&a.issued_at).then_with(|| a.grant_id.cmp(&b.grant_id)));
    Ok(views)
}

/// Withdraws one consent, atomically against the Grant it came from.
///
/// This is the same revocation the dispatch path observes: the reservation reads
/// the Grant inside its owning transaction, so once this commits no new call can
/// derive authority from the consent. Work that already reserved an Attempt under
/// it is left alone, which is the existing rule for a mode change or a withdrawal.
///
/// The returned view is the *withdrawn* consent. A withdrawal that committed and
/// then failed to describe itself would report a success as an error, which is how
/// the surface came to tell people their revocation had not been submitted.
pub fn revoke(
    journal: &crate::broker::BrokerJournal,
    permission_authority: &aura_persistence::PersistenceCoordinator,
    runtime: aura_domain_common::RuntimeInstanceId,
    input: &RevokeNativeSessionConsentInput,
) -> Result<NativeSessionConsentView, NativeSessionConsentError> {
    let now = UtcTimestamp::now();
    let live = live_facts(permission_authority, input.session_id, runtime)?;
    let workspace = {
        let store = journal.lock().map_err(|_| NativeSessionConsentError::Store)?;
        aura_persistence_v2::workspace::session(store.database.connection(), input.session_id)?
            .ok_or(NativeSessionConsentError::SessionNotFound)?
            .workspace_id
    };
    let (consent, approval_expires_at) = {
        let mut store = journal.lock().map_err(|_| NativeSessionConsentError::Store)?;
        let database = store.database.connection();
        let consent =
            aura_persistence_v2::security::native_session_consent(database, input.grant_id)?
                .ok_or(NativeSessionConsentError::NotFound)?;
        if consent.session_id != input.session_id {
            // A management action for another session must not withdraw this one.
            return Err(NativeSessionConsentError::NotFound);
        }
        let approval_expires_at =
            aura_persistence_v2::security_approval::load(database, consent.approval_id)
                .map(|approval| approval.expires_at)
                .unwrap_or(now);
        let reference = consent.reference();
        store.database.unit_of_work(|tx| {
            aura_persistence_v2::security::revoke(
                tx,
                input.grant_id,
                input.expected_revision,
                Some(&reference),
                now,
            )
        })?;
        (consent, approval_expires_at)
    };
    let store = journal.lock().map_err(|_| NativeSessionConsentError::Store)?;
    let database = store.database.connection();
    let grant = aura_persistence_v2::security::grant(database, input.grant_id)?
        .ok_or(NativeSessionConsentError::NotFound)?;
    let withdrawn =
        aura_persistence_v2::security::native_session_consent(database, input.grant_id)?
            .ok_or(NativeSessionConsentError::NotFound)?;
    let _ = consent;
    view(&withdrawn, &grant, workspace, approval_expires_at, live, now)
        .ok_or(NativeSessionConsentError::Unusable)
}

/// Reads the two live facts under the session's permission barrier.
fn live_facts(
    permission_authority: &aura_persistence::PersistenceCoordinator,
    session: SessionId,
    runtime: aura_domain_common::RuntimeInstanceId,
) -> Result<LiveConsentFacts, NativeSessionConsentError> {
    permission_authority
        .with_session_permission_authority(session, |current| LiveConsentFacts {
            core_runtime_instance_id: runtime,
            session_permission_revision: current.revision,
        })
        .map_err(|_| NativeSessionConsentError::Store)
}

/// Projects one stored consent, refusing a record that does not describe itself.
///
/// A consent whose stored Grant does not accept the row's revision is not shown at
/// all: an unreadable authorization must never be presented as one the user can rely
/// on, and it must not be silently downgraded to a weaker kind either.
#[must_use]
pub fn view(
    consent: &NativeSessionConsent,
    grant: &AuthorizationGrant,
    workspace_id: aura_domain_common::WorkspaceId,
    approval_expires_at: UtcTimestamp,
    live: LiveConsentFacts,
    now: UtcTimestamp,
) -> Option<NativeSessionConsentView> {
    if !consent.matches_grant(grant) {
        return None;
    }
    Some(NativeSessionConsentView {
        // The revision a person acts on is the Grant's *current* one: a withdrawal
        // has to compare-and-swap against it, so reporting the issued revision here
        // would make every withdrawal of an already-withdrawn consent a conflict.
        grant_revision: grant.revision,
        grant_id: consent.grant_id,
        approval_id: consent.approval_id,
        scope: NativeSessionConsentScopeView {
            lifetime: consent.lifetime,
            session_id: consent.session_id,
            workspace_id,
            approved_shell: consent.approved.approved_shell,
            shells: consent.approved.shells.clone(),
            principal_id: consent.approved.principal_id,
            approved_permission_revision: consent.permission_revision,
            core_runtime_instance_id: consent.runtime_instance,
            approved_operation_fingerprint: consent.approved_operation_fingerprint.clone(),
        },
        availability: availability(grant, consent, live, now),
        issued_at: grant.issued_at,
        approved_through: approval_expires_at,
        revoked_at: grant.revoked_at,
        backend_id: if consent.lifetime == aura_security_contract::ShellConsentLifetime::Session {
            aura_backend_windows_user::compat::COMPAT_BACKEND_ID
        } else {
            aura_backend_windows_user::NATIVE_BACKEND_ID
        }
        .to_owned(),
    })
}

/// The availability a person reads, from the Grant *and* the live Core facts.
///
/// The Grant answers "was this withdrawn or consumed". Core answers the two
/// questions the dispatch path also asks and the Grant cannot: whether this is still
/// the Core that issued the consent, and whether the session's permissions are still
/// the ones the consent was approved under. A consent that fails either is not in
/// force, and saying it is would contradict what dispatch does with the same facts.
///
/// The two failures read differently on purpose. A consent from an earlier Core run is
/// `Superseded` — the restart ended it and nothing will revive it. A consent approved
/// under different session permissions is `Expired`: the authority it was granted under
/// is gone. Neither is `NotYetValid`, which is reserved for a record whose own issue
/// time is still in the future.
#[must_use]
pub fn availability(
    grant: &AuthorizationGrant,
    consent: &NativeSessionConsent,
    live: LiveConsentFacts,
    now: UtcTimestamp,
) -> NativeSessionConsentAvailability {
    if grant.revoked_at.is_some() {
        return NativeSessionConsentAvailability::Revoked;
    }
    if grant.consumed_at.is_some() {
        return NativeSessionConsentAvailability::Consumed;
    }
    if grant.issued_at > now {
        return NativeSessionConsentAvailability::NotYetValid;
    }
    if grant.expires_at.is_some_and(|expires| now >= expires) {
        return NativeSessionConsentAvailability::Expired;
    }
    if consent.lifetime == aura_security_contract::ShellConsentLifetime::LegacyRuntime
        && consent.runtime_instance != live.core_runtime_instance_id
    {
        // A consent an earlier Core run issued. It is not "waiting to take effect": this
        // process will never inherit it, so the honest answer is that it has been
        // superseded by the restart.
        return NativeSessionConsentAvailability::Superseded;
    }
    if consent.lifetime == aura_security_contract::ShellConsentLifetime::LegacyRuntime
        && consent.permission_revision != live.session_permission_revision
    {
        return NativeSessionConsentAvailability::Expired;
    }
    NativeSessionConsentAvailability::InForce
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{
        ApprovalRequestId, PermissionGrantId, Revision, RuntimeInstanceId, WorkspaceId,
    };
    use aura_security_contract::{
        AuthorizationLifetime, AuthorizationScope, BackendClass, BackendRequirement, NativeShell,
        OperationFingerprint, SecurityProfile,
    };

    fn consent(runtime: RuntimeInstanceId, permission_revision: u64) -> NativeSessionConsent {
        NativeSessionConsent {
            lifetime: aura_security_contract::ShellConsentLifetime::LegacyRuntime,
            grant_id: PermissionGrantId::new(),
            revision: Revision::INITIAL,
            approval_id: ApprovalRequestId::new(),
            session_id: SessionId::new(),
            runtime_instance: runtime,
            permission_revision,
            approved: aura_security_contract::NativeSessionConsentScope::for_approved_shell(
                aura_domain_common::PrincipalId::new(),
                NativeShell::PowerShell,
            ),
            approved_operation_fingerprint: OperationFingerprint(
                aura_domain_common::jcs::CanonicalSha256HashV1::sha256(b"approved"),
            ),
            revoked_grant_revision: None,
        }
    }

    fn grant_for(consent: &NativeSessionConsent, issued_at: UtcTimestamp) -> AuthorizationGrant {
        AuthorizationGrant {
            grant_id: consent.grant_id,
            revision: consent.revision,
            scope: AuthorizationScope {
                operation_fingerprint: consent.approved_operation_fingerprint.clone(),
                profile: SecurityProfile::FullAccess,
                backend_requirement: BackendRequirement {
                    allowed_classes: [BackendClass::Native].into(),
                    guarantees: Default::default(),
                },
                lifetime: AuthorizationLifetime::Session { session_id: consent.session_id },
                consent: Some(consent.reference()),
                // This consent is about Shell commands, not about file targets, so it carries no
                // file consent. A scope with both would be ambiguous about what was approved.
                network_consent: None,
                capability_limit: None,
                memory_consent: None,
                mcp_consent: None,
                chrome_consent: None,
                file_consent: None,
            },
            issued_at,
            expires_at: Some(UtcTimestamp::from_value(
                issued_at.value() + time::Duration::minutes(1),
            )),
            revoked_at: None,
            consumed_at: None,
        }
    }

    fn live(consent: &NativeSessionConsent) -> LiveConsentFacts {
        LiveConsentFacts {
            core_runtime_instance_id: consent.runtime_instance,
            session_permission_revision: consent.permission_revision,
        }
    }

    #[test]
    fn a_withdrawn_consent_is_projected_as_revoked_rather_than_hidden() {
        let now = UtcTimestamp::now();
        let mut consent = consent(RuntimeInstanceId::new(), 3);
        let mut grant = grant_for(&consent, now);
        // What the Store writes on withdrawal: the Grant's revision moves and the
        // reference records where it moved to.
        let revoked_revision = Revision::new(2);
        consent.revoked_grant_revision = Some(revoked_revision);
        grant.revision = revoked_revision;
        grant.revoked_at = Some(now);
        grant.scope.consent = Some(consent.reference());
        let view = view(
            &consent,
            &grant,
            WorkspaceId::new(),
            consent_approval_expiry(now),
            live(&consent),
            now,
        )
        .expect("a withdrawn consent is still a fact worth showing");
        assert_eq!(view.availability, NativeSessionConsentAvailability::Revoked);
        assert_eq!(
            view.grant_revision, revoked_revision,
            "the revision a caller must present is the Grant's current one"
        );
    }

    #[test]
    fn an_older_core_runtime_consent_is_not_in_force() {
        let now = UtcTimestamp::now();
        let consent = consent(RuntimeInstanceId::new(), 3);
        let grant = grant_for(&consent, now);
        let this_process = LiveConsentFacts {
            core_runtime_instance_id: RuntimeInstanceId::new(),
            session_permission_revision: consent.permission_revision,
        };
        let view = view(
            &consent,
            &grant,
            WorkspaceId::new(),
            consent_approval_expiry(now),
            this_process,
            now,
        )
        .expect("the record still projects");
        assert!(
            !view.availability.is_in_force(),
            "a consent this Core did not issue is not in force, and the list must not say it is"
        );
    }

    #[test]
    fn a_changed_permission_revision_is_not_in_force() {
        let now = UtcTimestamp::now();
        let consent = consent(RuntimeInstanceId::new(), 3);
        let grant = grant_for(&consent, now);
        let changed = LiveConsentFacts {
            core_runtime_instance_id: consent.runtime_instance,
            session_permission_revision: 4,
        };
        let view =
            view(&consent, &grant, WorkspaceId::new(), consent_approval_expiry(now), changed, now)
                .expect("the record still projects");
        assert!(!view.availability.is_in_force());
    }

    #[test]
    fn a_session_consent_stays_in_force_across_core_restarts_and_mode_changes() {
        // D2: a new Session answer ends only on revocation, expiry or session deletion.
        let now = UtcTimestamp::now();
        let mut consent = consent(RuntimeInstanceId::new(), 3);
        consent.lifetime = aura_security_contract::ShellConsentLifetime::Session;
        let grant = grant_for(&consent, now);
        let restarted_and_changed = LiveConsentFacts {
            core_runtime_instance_id: RuntimeInstanceId::new(),
            session_permission_revision: 4,
        };
        let view = view(
            &consent,
            &grant,
            WorkspaceId::new(),
            consent_approval_expiry(now),
            restarted_and_changed,
            now,
        )
        .expect("the record still projects");
        assert_eq!(view.availability, NativeSessionConsentAvailability::InForce);
    }

    #[test]
    fn an_unrelated_grant_revision_is_refused_rather_than_projected() {
        let now = UtcTimestamp::now();
        let consent = consent(RuntimeInstanceId::new(), 3);
        let mut grant = grant_for(&consent, now);
        grant.revision = Revision::new(7);
        assert!(
            view(
                &consent,
                &grant,
                WorkspaceId::new(),
                consent_approval_expiry(now),
                live(&consent),
                now
            )
            .is_none(),
            "a revision this consent never held means the row and the Grant disagree"
        );
        grant.scope.consent.as_mut().expect("consent").grant_revision = grant.revision;
        assert!(
            view(
                &consent,
                &grant,
                WorkspaceId::new(),
                consent_approval_expiry(now),
                live(&consent),
                now
            )
            .is_none(),
            "the reference cannot redefine the persisted issuance revision"
        );
    }

    /// The production projection refuses a Grant whose consent binding was replaced.
    ///
    /// This is the management function itself, not a copy of it. A row and a Grant can
    /// agree about `grant_id` and the revision while describing two different
    /// authorizations; the Store's reservation compares the complete reference, so a
    /// view that only compared identifiers would advertise an authorization that
    /// dispatch refuses.
    #[test]
    fn a_rebound_consent_reference_is_not_projected_as_usable() {
        let now = UtcTimestamp::now();
        let consent = consent(RuntimeInstanceId::new(), 3);
        let grant = grant_for(&consent, now);
        assert!(
            view(
                &consent,
                &grant,
                WorkspaceId::new(),
                consent_approval_expiry(now),
                live(&consent),
                now
            )
            .is_some(),
            "the matching pair projects"
        );
        let mut rebound = grant.clone();
        let reference = rebound.scope.consent.as_mut().expect("the Grant carries the consent");
        reference.approved.principal_id = aura_domain_common::PrincipalId::new();
        assert!(
            view(
                &consent,
                &rebound,
                WorkspaceId::new(),
                consent_approval_expiry(now),
                live(&consent),
                now
            )
            .is_none(),
            "a different authorized principal must not be shown as this consent"
        );
    }

    /// A consent from an earlier Core run reads as superseded, not as pending.
    #[test]
    fn a_consent_from_an_earlier_core_run_is_superseded_rather_than_pending() {
        let now = UtcTimestamp::now();
        let consent = consent(RuntimeInstanceId::new(), 3);
        let grant = grant_for(&consent, now);
        let restarted = LiveConsentFacts {
            core_runtime_instance_id: RuntimeInstanceId::new(),
            session_permission_revision: consent.permission_revision,
        };
        let view = view(
            &consent,
            &grant,
            WorkspaceId::new(),
            consent_approval_expiry(now),
            restarted,
            now,
        )
        .expect("the record still projects");
        assert_eq!(view.availability, NativeSessionConsentAvailability::Superseded);
        assert!(!view.availability.is_in_force());
    }

    fn consent_approval_expiry(now: UtcTimestamp) -> UtcTimestamp {
        UtcTimestamp::from_value(now.value() + time::Duration::minutes(10))
    }
}
