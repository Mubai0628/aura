//! SQL ownership for exact, single-resolution approvals. No backend or UI
//! chooses a broader grant than the stored Core proposal.
use aura_domain_common::{
    ApprovalRequestId, PermissionGrantId, Revision, RuntimeInstanceId, UtcTimestamp,
};
use aura_execution_contract::SecuredOperation;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_security_contract::backend_class_can_hold_a_session_consent;
use aura_security_contract::*;
use rusqlite::{Connection, Transaction, params};

pub fn load(connection: &Connection, id: ApprovalRequestId) -> StoreResult<AuthorizationApproval> {
    let payload: String = connection.query_row(
        "SELECT payload FROM approval_requests WHERE id=?1",
        [id.to_string()],
        |r| r.get(0),
    )?;
    record::decode(&payload)
}

/// Close a pending proposal without granting any permission. Used by Agent
/// cancellation, expiry and live-authority invalidation.
pub fn close(
    tx: &Transaction<'_>,
    id: ApprovalRequestId,
    state: ApprovalState,
    now: UtcTimestamp,
) -> StoreResult<()> {
    if !matches!(
        state,
        ApprovalState::Cancelled | ApprovalState::Expired | ApprovalState::Invalidated
    ) {
        return Err(Error::Conflict);
    }
    let mut a = load(tx, id)?;
    if a.state != ApprovalState::Pending {
        return Ok(());
    }
    if now < a.requested_at || (state == ApprovalState::Expired && now < a.expires_at) {
        return Err(Error::Conflict);
    }
    let previous = a.revision;
    a.revision = Revision::new(previous.value().checked_add(1).ok_or(Error::Conflict)?);
    a.state = state;
    record::changed(tx.execute(
        "UPDATE approval_requests SET revision=?2,state=?3,payload=?4 WHERE id=?1 AND revision=?5",
        params![
            id.to_string(),
            record::sql_u64(a.revision.value())?,
            record::encode(&a.state)?,
            record::encode(&a)?,
            record::sql_u64(previous.value())?
        ],
    )?)?;
    super::security::audit(tx, &id.to_string(), "approval.closed", &a, now)
}

#[expect(
    clippy::too_many_arguments,
    reason = "One transaction binds the approval, the exact Operation, the backend the user answered for, the live session permission revision and the Core runtime instance that would own any reusable consent"
)]
pub fn resolve(
    tx: &Transaction<'_>,
    id: ApprovalRequestId,
    revision: Revision,
    operation: &SecuredOperation,
    choice: ApprovalDecisionKind,
    backend_class: aura_security_contract::BackendClass,
    session_permission_revision: u64,
    core_runtime_instance_id: RuntimeInstanceId,
    now: UtcTimestamp,
) -> StoreResult<Option<AuthorizationGrant>> {
    let mut approval = load(tx, id)?;
    operation.validate().map_err(|_| Error::Invalid("approval operation"))?;
    if approval.revision != revision
        || approval.state != ApprovalState::Pending
        || approval.requested_at > now
        || approval.expires_at <= now
        || approval.scope.operation_fingerprint != operation.fingerprint
        || !approval.allowed_decisions.contains(&choice)
        || !matches!(approval.scope.lifetime, AuthorizationLifetime::Once{operation_id} if operation_id==operation.operation_id)
    {
        return Err(Error::Conflict);
    }
    if session_permission_revision == 0 {
        return Err(Error::Invalid("approval permission revision"));
    }
    let identity = &operation.content.identity;
    let lifetime = match choice {
        ApprovalDecisionKind::Deny => None,
        ApprovalDecisionKind::AllowOnce => Some(approval.scope.lifetime.clone()),
        ApprovalDecisionKind::AllowSession => Some(AuthorizationLifetime::Session {
            session_id: identity.session_id.ok_or(Error::Conflict)?,
        }),
        ApprovalDecisionKind::AllowWorkspace => Some(AuthorizationLifetime::Workspace {
            workspace_id: identity.workspace_id.ok_or(Error::Conflict)?,
        }),
        ApprovalDecisionKind::AllowForCurrentTurn => Some(AuthorizationLifetime::TurnTask {
            session_id: identity.session_id.ok_or(Error::Conflict)?,
            turn_task_id: identity.turn_task_id.ok_or(Error::Conflict)?,
        }),
    };
    approval.revision = Revision::new(revision.value().checked_add(1).ok_or(Error::Conflict)?);
    approval.state =
        if lifetime.is_some() { ApprovalState::Approved } else { ApprovalState::Denied };
    record::changed(tx.execute(
        "UPDATE approval_requests SET revision=?2,state=?3,payload=?4 WHERE id=?1 AND revision=?5",
        params![
            id.to_string(),
            record::sql_u64(approval.revision.value())?,
            record::encode(&approval.state)?,
            record::encode(&approval)?,
            record::sql_u64(revision.value())?
        ],
    )?)?;
    super::security::audit(tx, &id.to_string(), "approval.resolved", &approval, now)?;
    let Some(lifetime) = lifetime else { return Ok(None) };
    // A session approval of an ordinary built-in Shell **on the Native backend** is
    // the one approval that deliberately outlives the Operation the user confirmed.
    // It becomes a reusable consent only when both the backend and the approved
    // Operation are ones a consent may cover; everything else keeps the
    // exact-operation meaning it has always had, including a session approval of any
    // other tool and a session approval of a restricted backend.
    let consent = match choice {
        ApprovalDecisionKind::AllowSession
            if backend_class_can_hold_a_session_consent(backend_class)
                && approval.scope.capability_limit.as_ref().is_none_or(|caps| {
                    caps.iter().any(|cap| {
                        matches!(
                            cap,
                            ResolvedCapability::ProcessExecute
                                | ResolvedCapability::DownloadArtifact
                        )
                    })
                }) =>
        {
            identity.session_id.and_then(|session_id| {
                native_session_consent_scope(
                    &operation.content.source,
                    operation.content.action_kind.as_str(),
                    identity.principal_id,
                )
                .map(|approved| super::security::NativeSessionConsent {
                    lifetime: aura_security_contract::ShellConsentLifetime::Session,
                    grant_id: PermissionGrantId::new(),
                    revision: Revision::INITIAL,
                    approval_id: id,
                    session_id,
                    runtime_instance: core_runtime_instance_id,
                    permission_revision: session_permission_revision,
                    approved,
                    approved_operation_fingerprint: operation.fingerprint.clone(),
                    revoked_grant_revision: None,
                })
            })
        }
        _ => None,
    };
    let mut scope = approval.scope;
    scope.lifetime = lifetime;
    if let Some(consent) = consent.as_ref() {
        scope.consent = Some(consent.reference());
        if scope.capability_limit.is_none() {
            scope.capability_limit = Some(vec![ResolvedCapability::ProcessExecute]);
        }
    }
    // A session approval of an ordinary **file** operation records the second reusable consent.
    //
    // This is the confirmed product rule: "allow for this session" on a file target means the
    // session may keep using that capability on that target, not that it may re-send the exact
    // request the user already saw. The consent is issued only when the operation really names
    // file targets and needs only file capabilities, so a session answer for a network,
    // configuration or system-management operation keeps its exact-operation meaning.
    //
    // The target identity is taken from the operation's own resolved resources — the canonical
    // forms classification verified — never from the model's spelling.
    let file_consent = match choice {
        ApprovalDecisionKind::AllowSession => identity.session_id.and_then(|session_id| {
            // The targets and the capability test come from the shared contract helpers, so the
            // consent recorded here and the coverage test applied to a later call cannot drift.
            if !aura_security_contract::file_capabilities_are_consentable(
                &operation.content.capabilities,
            ) {
                return None;
            }
            let mut targets = operation
                .content
                .resources
                .iter()
                .filter_map(aura_security_contract::file_consent_target_of)
                .collect::<Vec<_>>();
            if targets.is_empty() {
                return None;
            }
            targets.sort();
            targets.dedup();
            Some(aura_security_contract::FileSessionConsentRef {
                approval_id: id,
                session_id,
                principal_id: identity.principal_id,
                capabilities: file_consent_capabilities(&operation.content.capabilities),
                targets,
                approved_operation_fingerprint: operation.fingerprint.clone(),
                grant_revision: Revision::INITIAL,
                revoked_grant_revision: None,
            })
        }),
        _ => None,
    };
    if let Some(file) = file_consent.as_ref() {
        scope.file_consent = Some(file.clone());
    }
    if choice == ApprovalDecisionKind::AllowSession
        && operation.content.capabilities
            == [aura_security_contract::ResolvedCapability::MemoryWrite]
        && aura_security_contract::built_in_memory(
            &operation.content.source,
            &operation.content.action_kind,
            &operation.content.capabilities,
            &operation.content.resources,
        )
        && let [
            aura_security_contract::ResourceDescriptor::MemoryVault {
                scope: target,
                vault_identity,
            },
        ] = operation.content.resources.as_slice()
        && let Some(session_id) = identity.session_id
    {
        scope.memory_consent = Some(aura_security_contract::MemorySessionConsentRef {
            approval_id: id,
            session_id,
            principal_id: identity.principal_id,
            scope: target.clone(),
            vault_identity: vault_identity.clone(),
            approved_operation_fingerprint: operation.fingerprint.clone(),
            grant_revision: Revision::INITIAL,
            revoked_grant_revision: None,
        });
    }
    // A ReadOnly answer for an MCP call covers that server, at the configuration the user
    // trusted, for the rest of this turn.
    if choice == ApprovalDecisionKind::AllowForCurrentTurn
        && let Some((server_id, config_fingerprint)) = aura_security_contract::mcp_server_call(
            &operation.content.source,
            &operation.content.action_kind,
            &operation.content.capabilities,
            &operation.content.resources,
        )
        && let (Some(session_id), Some(turn_task_id)) = (identity.session_id, identity.turn_task_id)
    {
        scope.mcp_consent = Some(aura_security_contract::McpTurnConsentRef {
            approval_id: id,
            session_id,
            turn_task_id,
            principal_id: identity.principal_id,
            server_id: server_id.to_owned(),
            config_fingerprint: config_fingerprint.clone(),
            approved_operation_fingerprint: operation.fingerprint.clone(),
            grant_revision: Revision::INITIAL,
        });
    }
    // A ReadOnly answer for a Chrome call covers reading and acting in that shared tab for the rest
    // of this turn; a sensitive action and a site the user has not confirmed still ask.
    if choice == ApprovalDecisionKind::AllowForCurrentTurn
        && aura_security_contract::built_in_chrome(
            &operation.content.source,
            &operation.content.action_kind,
            &operation.content.capabilities,
            &operation.content.resources,
        )
        && operation.content.capabilities.iter().all(|capability| {
            matches!(
                capability,
                ResolvedCapability::ChromeRead | ResolvedCapability::ChromeInteraction
            )
        })
        && let Some(share_id) = aura_security_contract::chrome_share(&operation.content.resources)
        && let (Some(session_id), Some(turn_task_id)) = (identity.session_id, identity.turn_task_id)
    {
        scope.chrome_consent = Some(aura_security_contract::ChromeTurnConsentRef {
            approval_id: id,
            session_id,
            turn_task_id,
            principal_id: identity.principal_id,
            share_id: share_id.to_owned(),
            approved_operation_fingerprint: operation.fingerprint.clone(),
            grant_revision: Revision::INITIAL,
        });
    }
    let grant_id = consent.as_ref().map_or_else(PermissionGrantId::new, |value| value.grant_id);
    crate::network_consents::attach(&mut scope, operation, id, grant_id)?;
    let expires_at = if scope.file_consent.is_some()
        || scope.network_consent.is_some()
        || scope.memory_consent.is_some()
        || scope.mcp_consent.is_some()
        || scope.chrome_consent.is_some()
    {
        None
    } else {
        grant_expiry(consent.as_ref(), approval.expires_at)
    };
    let mut grant = AuthorizationGrant {
        grant_id,
        revision: Revision::INITIAL,
        scope,
        issued_at: now,
        expires_at,
        revoked_at: None,
        consumed_at: None,
    };
    if let Some(network) = crate::network_consents::split(&mut grant) {
        super::security::insert_grant(tx, &network)?;
    }
    super::security::insert_grant(tx, &grant)?;
    if let Some(consent) = consent {
        super::security::insert_native_session_consent(tx, &consent)?;
    }
    Ok(Some(grant))
}

/// When the Grant a resolution issues stops being usable.
///
/// Three deadlines exist and they are not the same deadline:
///
/// * the **confirmation window** (`approval.expires_at`) bounds how long the user has
///   to answer. It is a property of the question.
/// * the **reusable consent** is a property of this Core run: it ends when the
///   consent is withdrawn, when the session's permissions change, or when Core
///   restarts. None of those is a wall-clock instant, so a consent Grant carries no
///   fixed expiry. Attaching the confirmation window to it is what made an approval
///   given with one minute left valid for one minute, and made a legitimate
///   approval at the edge of the window unable to authorize the dispatch it was for.
/// * the **dispatch credential** is the short-lived `AuthorizationDecision`, which
///   every caller already bounds separately and which must lie inside this Grant.
///
/// Every other authorization keeps the confirmation window, which is the meaning it
/// has always had.
fn grant_expiry(
    consent: Option<&super::security::NativeSessionConsent>,
    approval_expires_at: UtcTimestamp,
) -> Option<UtcTimestamp> {
    match consent {
        Some(_) => None,
        None => Some(approval_expires_at),
    }
}
