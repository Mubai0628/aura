//! V2 control plane. No process handles, Sandbox Plan, database or readiness
//! probes enter assessment. Core supplies authenticated, freshly resolved facts.
use aura_domain_common::{ContentHash, RuntimeInstanceId, UtcTimestamp};
mod approval_capabilities;
mod backend;
mod deletion;
mod git;
mod once;
mod outside_writes;
use approval_capabilities::requested_capabilities;
use aura_execution_contract::{BackendDescriptor, OperationNetwork, SecuredOperation};
use aura_security_contract::*;
pub use backend::required_for_backend;
use std::collections::BTreeSet;

#[derive(Clone)]
pub struct PolicyInput<'a> {
    /// Derived by the Core entry point from the registered owner. Never decoded
    /// from an Agent request or inferred from a tool's self-declared metadata.
    pub network_caller: NetworkCallerKind,
    pub operation: &'a SecuredOperation,
    pub snapshot: &'a PermissionSnapshot,
    pub profile: SecurityProfile,
    pub source_enabled: bool,
    pub trust: &'a TrustState,
    pub source_fingerprint: Option<&'a ContentHash>,
    /// Result of the owning credential/network/release boundary checks, not a
    /// Renderer flag. FullAccess and Grants cannot override a failed boundary.
    pub hard_boundaries_satisfied: bool,
    pub denied_for_turn: bool,
    pub grants: &'a [AuthorizationGrant],
    pub verified_external_capabilities: &'a [ResolvedCapability],
    pub verified_external_grants: &'a [VerifiedExternalCapability],
    pub now: UtcTimestamp,
    pub expires_at: UtcTimestamp,
    /// This Core process, when the caller can name it. A reusable Native session
    /// consent records the instance that issued it, so restarting Core cannot
    /// resurrect one; the Store checks the same fact again at reservation. A
    /// caller that cannot name a runtime instance is treated as a different one,
    /// which is why the field being absent fails closed rather than open.
    pub core_runtime_instance_id: Option<RuntimeInstanceId>,
}

pub struct PolicyAssessment {
    pub requirement: BackendRequirement,
    pub asks: bool,
    pub forced_once: bool,
}

// UNWIRED: the SecurityProfile::Strict and ::Guided branches in here are never taken; only Balanced and FullAccess are produced.
pub fn assess(input: &PolicyInput<'_>) -> Result<PolicyAssessment, PolicyDenyReason> {
    use PolicyDenyReason as D;
    let op = input.operation;
    op.validate().map_err(|_| D::InvalidOperation)?;
    let c = &op.content;
    let id = &c.identity;
    let memory = built_in_memory(&c.source, &c.action_kind, &c.capabilities, &c.resources);
    if has_memory_authority(&c.action_kind, &c.capabilities, &c.resources) && !memory {
        return Err(D::InvalidOperation);
    }
    if c.action_kind == "memory.repair" {
        let input = c.action.get("input").unwrap_or(&c.action);
        let expected = match input["phase"].as_str() {
            Some("preview") => ResolvedCapability::MemoryRead,
            Some("apply") => ResolvedCapability::MemoryWrite,
            _ => return Err(D::InvalidOperation),
        };
        if c.capabilities != [expected] {
            return Err(D::InvalidOperation);
        }
    }
    if c.resources.iter().any(|r|matches!(r,ResourceDescriptor::MemoryVault {scope:MemoryVaultScope::Project {workspace_id},..} if *workspace_id!=input.snapshot.workspace_id)) {return Err(D::IdentityMismatch);}
    if id.workspace_id != Some(input.snapshot.workspace_id)
        || id.session_id != Some(input.snapshot.session_id)
        || id.permission_snapshot_id != Some(input.snapshot.id)
        || input.snapshot.session_permission_revision == 0
        || input.snapshot.created_at > input.now
        || input.expires_at <= input.now
        || match id.actor {
            InvocationActor::User | InvocationActor::SystemRuntime => {
                id.parent_agent_run_id.is_some()
            }
            InvocationActor::ParentAgent => {
                id.agent_run_id.is_none() || id.parent_agent_run_id.is_some()
            }
            InvocationActor::ChildAgent => {
                id.agent_run_id.is_none() || id.parent_agent_run_id.is_none()
            }
        }
    {
        return Err(D::IdentityMismatch);
    }
    if !input.source_enabled {
        return Err(D::SourceDisabled);
    }
    if c.source_revision_fingerprint.as_ref() != input.source_fingerprint {
        return Err(D::SourceUntrusted);
    }
    if matches!(
        c.source,
        SecuritySourceRef::Hook { .. }
            | SecuritySourceRef::McpServer { .. }
            | SecuritySourceRef::Skill { .. }
            | SecuritySourceRef::Plugin { .. }
    ) && c.source_revision_fingerprint.is_none()
    {
        return Err(D::SourceUntrusted);
    }
    if c.resources.iter().any(|r|matches!(r,ResourceDescriptor::WorkspaceRoot{workspace_id}|ResourceDescriptor::WorkspacePath{workspace_id,..} if *workspace_id!=input.snapshot.workspace_id)) {return Err(D::IdentityMismatch);}
    if c.resources.iter().any(|r| matches!(r, ResourceDescriptor::AgentTask { session_id, turn_task_id, agent_run_id }
        if Some(*session_id) != id.session_id || Some(*turn_task_id) != id.turn_task_id || Some(*agent_run_id) != id.agent_run_id)) { return Err(D::IdentityMismatch); }
    if !c.source.trust_is_effective(input.source_fingerprint, input.trust) {
        return Err(D::SourceUntrusted);
    }
    // An MCP server source acts only in the exact MCP call shape, and the coarse MCP
    // capability belongs to that shape alone; nothing else may borrow it.
    let mcp = aura_security_contract::mcp_server_call(
        &c.source,
        &c.action_kind,
        &c.capabilities,
        &c.resources,
    )
    .is_some();
    if (matches!(c.source, SecuritySourceRef::McpServer { .. })
        || c.capabilities.contains(&ResolvedCapability::ExternalExtensionCall))
        && (!mcp || c.network != OperationNetwork::None)
    {
        return Err(D::IdentityMismatch);
    }
    if !input.hard_boundaries_satisfied {
        return Err(D::HardBoundary);
    }
    if input.denied_for_turn {
        return Err(D::DeniedForTurn);
    }
    let native_git = built_in_native_git(&c.source, &c.action_kind, &c.capabilities);
    // The built-in browser decides with its own site policy; only its exact shape qualifies,
    // and nothing else may use the browser's network declaration.
    let browser = crate::browser::shape(input);
    if (aura_security_contract::browser_tool(&c.source, &c.action_kind)
        || c.network == OperationNetwork::BrowserSitePolicy)
        && !browser
    {
        return Err(D::InvalidOperation);
    }
    // An installed, trusted package program is the terminal-like Native exception
    //. Only its exact shape qualifies.
    let package_program = aura_security_contract::package_program(
        &c.source,
        &c.action_kind,
        &c.capabilities,
        &c.resources,
    );
    let process = native_git
        || c.capabilities.iter().any(|v| {
            matches!(
                v,
                ResolvedCapability::ProcessExecute
                    | ResolvedCapability::InteractiveTerminal
                    | ResolvedCapability::BackgroundProcess
            )
        });
    let network = browser && c.network != OperationNetwork::None
        || !browser
            && c.capabilities.iter().any(|v| {
                matches!(
                    v,
                    ResolvedCapability::OrdinaryNetwork
                        | ResolvedCapability::PublicNetworkRead
                        | ResolvedCapability::PrivateNetworkAccess
                        | ResolvedCapability::DataEgress
                        | ResolvedCapability::GitRemoteRead
                        | ResolvedCapability::GitRemoteWrite
                        | ResolvedCapability::DownloadArtifact
                        | ResolvedCapability::DownloadWindowsExecutable
                        | ResolvedCapability::ExternalServiceRead
                        | ResolvedCapability::ExternalServiceMutation
                        | ResolvedCapability::ExternalServiceDestructiveMutation
                        | ResolvedCapability::BrowserRead
                        | ResolvedCapability::BrowserInteraction
                        | ResolvedCapability::BrowserSensitiveAction
                )
            });
    if network && c.network == OperationNetwork::None
        || c.network == OperationNetwork::BroadProcess && !process
        || c.network == OperationNetwork::ExactOrigins
            && !c.resources.iter().any(|r| matches!(r, ResourceDescriptor::NetworkOrigin { .. }))
    {
        return Err(D::NetworkDeclaration);
    }
    let ordinary = crate::network::is_ordinary(input);
    let shell = built_in_shell(&c.source, &c.action_kind);
    if c.capabilities.contains(&ResolvedCapability::OrdinaryNetwork) && !ordinary {
        return Err(D::NetworkDeclaration);
    }
    let controlled_web = ordinary && !shell && !package_program
        || input.network_caller == NetworkCallerKind::WebRuntime
            && matches!(c.source, SecuritySourceRef::BuiltInTool { .. })
            && matches!(c.action_kind.as_str(), "web.search" | "web.fetch" | "web.read")
            && !process
            && c.capabilities.iter().all(|v| *v == ResolvedCapability::PublicNetworkRead)
            && c.network == OperationNetwork::ExactOrigins;
    // The session mode is a **default** for *file* authority, so falling outside it is a
    // reason to ask rather than a refusal: that is how a ReadOnly session approves writing
    // one named target without changing its mode. Two things stay absolute:
    //
    // * the capabilities no approval may express (the user's configuration control plane,
    //   and a child agent acting on authority only its parent established), and
    // * **network**, which is not an extension of file authority at all. A mode that does
    //   not reach the network keeps refusing it, because approving network access is a
    //   separate capability and a separate stage — a file approval must never become
    //   a route to it, and the confirmed rule is that network does not confer file
    //   capability and file capability does not confer network.
    let mode_permits = git::mode_permits(
        input,
        &c.capabilities,
        network || c.network != OperationNetwork::None,
        controlled_web,
    );
    // Network is the one capability family a *file* approval must never become a route to.
    // Approving network access is its own capability with its own stage and its own dialog
    //, and the confirmed rule is symmetric: network does not confer file authority and
    // file authority does not confer network. So a mode that does not reach the network keeps
    // refusing a networked operation rather than offering it for a file-shaped approval.
    // Everything else outside the mode's default — including every file capability — asks.
    let networked = network
        || c.network != OperationNetwork::None
        || c.capabilities.iter().any(|capability| {
            crate::mode_ceiling::ordinary_network(*capability)
                || matches!(
                    capability,
                    ResolvedCapability::PublicNetworkRead
                        | ResolvedCapability::DownloadWindowsExecutable
                )
        });
    if !mode_permits
        && (crate::mode_ceiling::refuses_outright(id.actor, &c.capabilities)
            || networked && !ordinary && !native_git && !browser)
    {
        return Err(D::ModeDenied);
    }
    let mut classes: BTreeSet<_> = if process {
        [
            BackendClass::RestrictedUserOffline,
            BackendClass::RestrictedUserOnline,
            BackendClass::AppContainer,
            BackendClass::Container,
            BackendClass::Native,
        ]
        .into()
    } else {
        [BackendClass::CoreBroker, BackendClass::AppContainer, BackendClass::Container].into()
    };
    if c.network != OperationNetwork::None {
        classes.remove(&BackendClass::RestrictedUserOffline);
    }
    if memory {
        classes.retain(|class| *class == BackendClass::CoreBroker);
    }
    if c.network == OperationNetwork::None {
        classes.remove(&BackendClass::RestrictedUserOnline);
    }
    if matches!(id.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
        && input.snapshot.mode != PermissionMode::FullAccess
        && !shell
        && !native_git
        && !package_program
    {
        // Native cannot enforce the low-mode filesystem or network ceiling.
        classes.remove(&BackendClass::Native);
    }
    if input.profile == SecurityProfile::Strict {
        classes.retain(|c| {
            matches!(
                c,
                BackendClass::CoreBroker | BackendClass::AppContainer | BackendClass::Container
            )
        });
        if process && c.network != OperationNetwork::None {
            return Err(D::ProfileProhibited);
        }
    }
    let mut guarantees = BTreeSet::new();
    if memory {
        guarantees.extend([
            EnforcementGuarantee::AuthorizedReadsOnly,
            EnforcementGuarantee::AuthorizedWritesOnly,
            EnforcementGuarantee::BoundedOutput,
            EnforcementGuarantee::CredentialIsolation,
        ]);
    }
    if !c.credentials.is_empty() {
        guarantees.insert(EnforcementGuarantee::CredentialIsolation);
    }
    if c.network == OperationNetwork::ExactOrigins {
        guarantees.insert(EnforcementGuarantee::ExactNetworkOrigins);
    }
    // Native is an explicit exception to OS confinement, never to credentials
    // or exact-origin guarantees. Its exception is handled during authorization.
    if process {
        guarantees.extend([
            EnforcementGuarantee::ChildProcessControl,
            EnforcementGuarantee::BoundedOutput,
        ]);
        if input.snapshot.mode == PermissionMode::ReadOnly
            && !shell
            && !native_git
            && !package_program
            && matches!(id.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
        {
            guarantees.insert(EnforcementGuarantee::ReadOnlyUserFiles);
        }
    }
    if input.profile == SecurityProfile::Strict {
        guarantees.extend([
            EnforcementGuarantee::AuthorizedReadsOnly,
            EnforcementGuarantee::AuthorizedWritesOnly,
        ]);
        if process {
            guarantees.insert(EnforcementGuarantee::IdentityIsolation);
        }
    }
    let forced_once = once::forced_once(op, input.snapshot.mode);
    let direct_user =
        c.source == SecuritySourceRef::UserInterface && c.identity.actor == InvocationActor::User;
    let network_allowed = controlled_web || git::network_allowed(input);
    // An external capability is a reason to ask only when the operation reaches an external
    // object the *policy* decides about. A session grant is the user's own authorization for
    // the object it covers — it was created through the approval flow, it carries its own
    // scope, depth and capabilities, and Core re-checks it (including revocation) when the
    // handler resolves the target. Asking again for every call under a grant the user
    // already approved would be a second approval for the same object, and refusing the call
    // while it waits would make a granted external target unusable in the lower modes.
    let external_path_resource = c.resources.iter().any(|resource| {
        matches!(
            resource,
            ResourceDescriptor::ExternalPath { .. }
                | ResourceDescriptor::SystemPath { .. }
                | ResourceDescriptor::SensitiveUserPath { .. }
        )
    });
    let capability_asks_here = |capability: ResolvedCapability| {
        capability_asks(capability, input.snapshot.mode)
            && !input.verified_external_capabilities.contains(&capability)
            && (external_path_resource || !is_external_capability(capability))
    };
    // A resource that is **already an authorization** rather than a target the policy decides
    // about. The `ExternalGrant` descriptor is produced by Core's trusted resolver from a live
    // grant record — it is the user's own earlier approval, re-checked for capability, scope,
    // root identity, depth, term and revocation on the call path before this point. The mode's
    // default must not re-ask for it: the whole point of an approval is that the approved
    // object stops being decided by the default, which is what lets a ReadOnly session use a
    // target it explicitly approved. The record's own checks still refuse a missing, expired,
    // revoked, replaced or other-session grant before dispatch.
    let grant_authorized = c.capabilities.iter().all(|capability| {
        input.verified_external_capabilities.contains(capability)
            || git::mode_permits(
                input,
                &[*capability],
                network || c.network != OperationNetwork::None,
                controlled_web,
            )
    });
    let asks = forced_once
        // The session mode is a default. An operation the mode does not cover asks — unless a
        // live grant already covers it, in which case the user has already answered for this
        // object and capability. `refuses_outright` has already removed what no approval may
        // reach.
        || (!mode_permits && !grant_authorized)
        // A risk flag is a reason to ask when it names something the *default* does not cover.
        // `OutsideWorkspaceDelete` is deliberately not one of them any more: deleting inside a
        // writable scope is an ordinary write in that scope, and the confirmed rule says it
        // must not add a second question. The flags that remain are the ones no directory scope
        // can express — installing, elevating, changing the machine, or destroying a remote.
        || c.risks.iter().any(|r| {
            !(network_allowed
                && matches!(
                    r,
                    OperationRiskFlag::DataLeavesDevice
                        | OperationRiskFlag::PrivateOrLoopbackNetwork
                ))
                && operation_risk_asks_here(*r)
        })
        || !direct_user && c.capabilities.iter().any(|v| capability_asks_here(*v))
        || input.profile == SecurityProfile::Guided
            && !memory
            && input.snapshot.mode != PermissionMode::FullAccess
            && c.capabilities.iter().any(|c| {
                !read_capability(*c)
                    && !input.verified_external_capabilities.contains(c)
                    && !(network_allowed && crate::mode_ceiling::ordinary_network(*c))
                    && (external_path_resource || !is_external_capability(*c))
            })
        || c.resources.iter().any(|r| {
            // An ordinary external path is not by itself a reason to ask: the confirmed
            // product behaviour is that FullAccess follows the existing policy for an
            // ordinary external file without the user pre-creating a grant per
            // directory. The capability baseline already decides the modes, and system and
            // sensitive paths keep asking here.
            matches!(
                r,
                ResourceDescriptor::SystemPath { .. }
                    | ResourceDescriptor::SensitiveUserPath { .. }
            )
        });
    Ok(PolicyAssessment {
        requirement: BackendRequirement { allowed_classes: classes, guarantees },
        asks,
        forced_once,
    })
}

pub fn authorize(
    input: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    previous: Option<&AuthorizationDecision>,
) -> PolicyAuthorization {
    let assessment = match assess(input) {
        Ok(v) => v,
        Err(reason) => return PolicyAuthorization::Deny { reason },
    };
    let required = required_for_backend(&assessment, input.operation, backend.class);
    if !backend.supports_operation(&required, input.operation) {
        return PolicyAuthorization::Deny { reason: PolicyDenyReason::BackendUnavailable };
    }
    if crate::network::uses_capability_slices(input) {
        return crate::network::authorize(input, backend, &assessment);
    }
    let restricted_lifetime = backend.class == BackendClass::Native
        && !built_in_native_git(
            &input.operation.content.source,
            &input.operation.content.action_kind,
            &input.operation.content.capabilities,
        );
    let matches = |g: &&AuthorizationGrant| {
        g.is_live_at(input.now)
            && (g.scope.file_consent.is_some() || g.scope.profile == input.profile)
            && grant_covers_operation(g, input)
            && lifetime_matches(&g.scope.lifetime, input.operation)
            && (!assessment.forced_once
                || matches!(g.scope.lifetime, AuthorizationLifetime::Once { .. }))
            && (assessment.forced_once
                || restricted_lifetime
                || !matches!(
                    input.operation.content.source,
                    SecuritySourceRef::McpServer { .. } | SecuritySourceRef::Plugin { .. }
                )
                || matches!(g.scope.lifetime, AuthorizationLifetime::TurnTask { .. }))
            && (!restricted_lifetime
                || matches!(
                    g.scope.lifetime,
                    AuthorizationLifetime::Once { .. } | AuthorizationLifetime::Session { .. }
                ))
            && backend.supports_operation(&g.scope.backend_requirement, input.operation)
            && previous.is_none_or(|p| {
                backend.supports_operation(&p.scope.backend_requirement, input.operation)
                    || g.issued_at > p.decided_at
                        && p.basis
                            != (AuthorizationBasis::Grant {
                                grant_id: g.grant_id,
                                revision: g.revision,
                            })
            })
            && g.expires_at.is_none_or(|t| input.expires_at <= t)
    };
    if let Some(g) = input.grants.iter().find(matches)
        && let Some(authorization) = AuthorizationDecision::from_grant_for_operation(
            g,
            &input.operation.fingerprint,
            input.now,
            input.expires_at,
        )
    {
        return PolicyAuthorization::Allow { authorization };
    }
    let mut requirement = required;
    // Preserve every actual approved guarantee so future binding cannot silently
    // discard a dimension which happened not to be the policy minimum.
    requirement.guarantees.extend(backend.guarantees.iter().copied());
    if backend.class != BackendClass::Native {
        requirement.allowed_classes.remove(&BackendClass::Native);
    }
    if backend.class != BackendClass::RestrictedUserOnline {
        requirement.allowed_classes.remove(&BackendClass::RestrictedUserOnline);
    }
    let scope = AuthorizationScope {
        operation_fingerprint: input.operation.fingerprint.clone(),
        profile: input.profile,
        backend_requirement: requirement,
        lifetime: AuthorizationLifetime::Once { operation_id: input.operation.operation_id },
        // A proposal always authorizes exactly one Operation. Reuse is only ever
        // conferred afterwards, by the approval resolution that records a
        // reusable consent on the Grant it issues.
        consent: None,
        // The same is true of the file consent: the proposal carries none, and the resolution
        // records one when the user chose a session answer.
        network_consent: None,
        capability_limit: None,
        memory_consent: None,
        mcp_consent: None,
        file_consent: None,
    };
    let changed = previous.is_some_and(|p| {
        p.scope.profile != input.profile
            || p.scope.operation_fingerprint != input.operation.fingerprint
            || !backend.supports_operation(&p.scope.backend_requirement, input.operation)
    });
    if assessment.asks || restricted_lifetime || changed {
        let mut reasons = vec!["policy.approval_required".into()];
        if changed {
            reasons.push("backend.guarantees_changed".into());
        }
        if backend.class == BackendClass::Native {
            reasons.push("backend.native_explicit".into());
            reasons.push("execution.isolation_reduced".into());
        }
        let mut allowed_decisions =
            vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny];
        // `/init` writes one reviewed set of files; the answer covers exactly that set and
        // never becomes reusable authority for later writes.
        let once_only =
            matches!(input.operation.content.source, SecuritySourceRef::BuiltInTool { .. })
                && input.operation.content.action_kind == "workspace_init.complete";
        if !assessment.forced_once && !once_only {
            allowed_decisions.insert(1, ApprovalDecisionKind::AllowSession);
        }
        if !assessment.forced_once
            && !once_only
            && !restricted_lifetime
            && !input.operation.content.action_kind.starts_with("memory.")
        {
            if matches!(
                input.operation.content.source,
                SecuritySourceRef::McpServer { .. } | SecuritySourceRef::Plugin { .. }
            ) {
                allowed_decisions =
                    vec![ApprovalDecisionKind::AllowForCurrentTurn, ApprovalDecisionKind::Deny];
            } else {
                allowed_decisions.insert(2, ApprovalDecisionKind::AllowWorkspace);
            }
        }
        // A session approval of a Native Shell is the one answer that creates a
        // reusable consent, and only when the approved Operation is one a consent
        // can cover. The dialog is told exactly that, so it never has to describe a
        // range the approval would not actually produce. `approved_shell` is what the
        // user confirmed; `shells` is the range the answer covers.
        let native_session_consent = if restricted_lifetime
            && !assessment.forced_once
            && allowed_decisions.contains(&ApprovalDecisionKind::AllowSession)
        {
            native_session_consent_scope(input.operation).map(|approved| NativeSessionConsentFact {
                shell: approved.approved_shell,
                shells: approved.shells.clone(),
                principal_id: approved.principal_id,
                session_permission_revision: input.snapshot.session_permission_revision,
            })
        } else {
            None
        };
        return PolicyAuthorization::Ask {
            proposal: AuthorizationProposal {
                scope,
                allowed_decisions,
                reasons,
                requested_capabilities: Some(requested_capabilities(input)),
                native_session_consent,
            },
        };
    }
    PolicyAuthorization::Allow {
        authorization: AuthorizationDecision {
            basis: AuthorizationBasis::PermissionMode {
                snapshot_id: input.snapshot.id,
                mode: input.snapshot.mode,
                session_permission_revision: input.snapshot.session_permission_revision,
            },
            scope,
            decided_at: input.now,
            expires_at: input.expires_at,
        },
    }
}

pub fn lifetime_matches(lifetime: &AuthorizationLifetime, op: &SecuredOperation) -> bool {
    let id = &op.content.identity;
    match *lifetime {
        AuthorizationLifetime::Once { operation_id } => operation_id == op.operation_id,
        AuthorizationLifetime::Session { session_id } => id.session_id == Some(session_id),
        AuthorizationLifetime::TurnTask { session_id, turn_task_id } => {
            id.session_id == Some(session_id) && id.turn_task_id == Some(turn_task_id)
        }
        AuthorizationLifetime::Workspace { workspace_id } => id.workspace_id == Some(workspace_id),
    }
}

/// Whether a stored Grant authorizes *this* Operation.
///
/// The answer is one of the reuse tests, and each is deliberately narrower than "the
/// capabilities look similar":
///
/// * an exact-operation Grant requires the historical fingerprint comparison;
/// * a reusable Shell consent requires principal, session and Shell range;
///   only legacy records also require the issuing runtime and permission revision;
/// * reusable file consent checks every capability and target against its binding.
///
/// Nothing here widens a consent: it can only fail closed.
/// Live explicit approval for an existing managed execution. Consumption of its own Once
/// approval does not revoke that execution; it still cannot authorize another operation.
pub fn explicit_grant_covers_operation(
    grant: &AuthorizationGrant,
    input: &PolicyInput<'_>,
) -> bool {
    grant.revoked_at.is_none()
        && grant.issued_at <= input.now
        && grant.expires_at.is_none_or(|expiry| input.now < expiry)
        && (grant.consumed_at.is_none() || grant.scope.is_exact_operation())
        && lifetime_matches(&grant.scope.lifetime, input.operation)
        && grant_covers_operation(grant, input)
}

fn grant_covers_operation(grant: &AuthorizationGrant, input: &PolicyInput<'_>) -> bool {
    if grant.scope.mcp_consent.is_some() {
        let c = &input.operation.content;
        return aura_security_contract::mcp_consent_covers(
            grant,
            &c.identity,
            &c.source,
            &c.action_kind,
            &c.capabilities,
            &c.resources,
        );
    }
    if grant.scope.memory_consent.is_some() {
        let c = &input.operation.content;
        return memory_consent_covers(
            grant,
            &c.identity,
            &c.source,
            &c.action_kind,
            &c.capabilities,
            &c.resources,
        );
    }
    if grant.scope.network_consent.is_some() || grant.scope.capability_limit.is_some() {
        let c = &input.operation.content;
        return c.capabilities.iter().all(|cap| {
            aura_security_contract::grant_covers_capability(
                grant,
                &input.operation.fingerprint,
                &c.identity,
                &c.source,
                &c.action_kind,
                *cap,
                &c.resources,
            )
        });
    }
    if let Some(file) = grant.scope.file_consent.as_ref() {
        return file_consent_covers_operation(file, grant, input);
    }
    let Some(consent) = grant.scope.consent.as_ref() else {
        return grant.scope.operation_fingerprint == input.operation.fingerprint;
    };
    if consent.grant_id != grant.grant_id || consent.grant_revision != grant.revision {
        // A consent reference that does not describe the Grant carrying it is
        // corrupt, not a reason to fall back to a broader reading.
        return false;
    }
    // A caller that cannot name its own Core runtime instance never matches a
    // consent that names one.
    if consent.lifetime == ShellConsentLifetime::LegacyRuntime
        && input.core_runtime_instance_id != Some(consent.core_runtime_instance_id)
    {
        return false;
    }
    // Permissions may not have changed since the consent was approved. A
    // tightening followed by a return to the same mode advances the revision and
    // therefore invalidates the consent, which is what makes "downgrade then
    // upgrade" fail closed instead of resurrecting an old approval.
    if consent.lifetime == ShellConsentLifetime::LegacyRuntime
        && consent.session_permission_revision != input.snapshot.session_permission_revision
    {
        return false;
    }
    let identity = &input.operation.content.identity;
    if identity.principal_id != consent.approved.principal_id
        || !matches!(
            grant.scope.lifetime,
            AuthorizationLifetime::Session { session_id } if identity.session_id == Some(session_id)
        )
    {
        return false;
    }
    if !matches!(input.operation.content.source, SecuritySourceRef::BuiltInTool { .. }) {
        return false;
    }
    // The covered range decides, not the Shell the user happened to confirm first.
    // PowerShell and CMD are two spellings of "an ordinary Shell command in this
    // conversation", which is the question the user answered; `approved_shell` stays
    // on the consent as the audit record of what they saw.
    NativeShell::from_action_kind(input.operation.content.action_kind.as_str())
        .is_some_and(|shell| consent.approved.covers(shell))
}

/// Whether a reusable file consent authorizes *this* Operation.
///
/// # What this must not become
///
/// Not "the capabilities intersect" and not "the paths look similar". The consent is the record
/// of one answer the user gave, so every binding it carries is checked, and the operation has to
/// be **covered** by what was approved rather than merely adjacent to it:
///
/// * the reference must describe the Grant carrying it (a corrupt row is a refusal, not a reason
///   to read it more broadly);
/// * the session and the principal must be the ones the user answered for — another session or
///   another actor fails closed. A Core restart or a permission-mode change (loosening or
///   tightening) does **not** withdraw the answer; only revocation,
///   expiry or deletion of the session does;
/// * the answer must be a session answer (a `Once` grant never becomes reusable);
/// * **every** capability this operation needs must be one the user granted, so a read answer
///   never becomes a write answer;
/// * **every** target this operation names must be one the user named, compared by verified
///   object identity (or verified parent and native entry key for a create) and reached through
///   verified ancestors for a recursive answer — never by folded path text. An operation that
///   names an object the consent does not cover is not covered, even when it also names ones
///   that are.
///
/// The operation still gets its own fingerprint, its own preparation and its own dispatch
/// binding; this only decides that the fingerprint may descend from the approval.
fn file_consent_covers_operation(
    file: &aura_security_contract::FileSessionConsentRef,
    grant: &AuthorizationGrant,
    input: &PolicyInput<'_>,
) -> bool {
    if file.grant_revision != grant.revision || !file.accepts_grant_revision(grant.revision) {
        return false;
    }
    let identity = &input.operation.content.identity;
    if identity.principal_id != file.principal_id
        || !matches!(
            grant.scope.lifetime,
            AuthorizationLifetime::Session { session_id }
                if identity.session_id == Some(session_id) && session_id == file.session_id
        )
    {
        return false;
    }
    // Every capability the operation needs must be one the user granted. A subset check in this
    // direction is what makes "read answer" unable to become "write answer".
    if !input
        .operation
        .content
        .capabilities
        .iter()
        .all(|capability| file.capabilities.contains(capability))
    {
        return false;
    }
    // Every target the operation names must be one the consent covers. An operation with no
    // targetable resource is not a file reuse at all, so it is refused rather than allowed by
    // vacuous truth.
    let mut named = 0_usize;
    for resource in &input.operation.content.resources {
        let Some(candidate) = consent_target_of(resource) else {
            continue;
        };
        named += 1;
        if !file.targets.iter().any(|target| target_covers(target, &candidate)) {
            return false;
        }
    }
    named > 0
}

/// The canonical target identity a resource descriptor names, when it names a file object.
///
/// The derivation lives in `aura-security-contract` and is imported rather than reimplemented,
/// because the **writer** (the approval resolution recording what the user approved) and the
/// **reader** (this coverage test) must agree exactly. Two independent derivations would be two
/// chances to disagree about what was approved.
use aura_security_contract::{
    file_consent_target_covers as target_covers, file_consent_target_of as consent_target_of,
};

/// The reusable file consent one approval may record, when the Operation is one a file consent
/// can outlive.
///
/// It is the exact test [`file_consent_covers_operation`] applies later, so an approval can never
/// issue a consent that no later call could use. Only ordinary file capabilities qualify: a
/// network, configuration or system-management capability is never covered by a file consent.
#[must_use]
pub fn file_session_consent_targets(
    op: &SecuredOperation,
) -> Option<Vec<aura_security_contract::FileConsentTarget>> {
    if !aura_security_contract::file_capabilities_are_consentable(&op.content.capabilities) {
        return None;
    }
    let mut targets = Vec::new();
    for resource in &op.content.resources {
        let Some(target) = consent_target_of(resource) else {
            continue;
        };
        targets.push(target);
    }
    if targets.is_empty() {
        return None;
    }
    targets.sort();
    targets.dedup();
    Some(targets)
}

/// Whether a Native session consent may be minted for this Operation.
///
/// The approval itself is the authority; this only decides whether the approved
/// Operation is one a consent is allowed to outlive. It is deliberately the same
/// test [`grant_covers_operation`] applies later, so an approval can never issue
/// a consent that no later call could use.
#[must_use]
pub fn native_session_consent_scope(op: &SecuredOperation) -> Option<NativeSessionConsentScope> {
    aura_security_contract::native_session_consent_scope(
        &op.content.source,
        op.content.action_kind.as_str(),
        op.content.identity.principal_id,
    )
}

fn read_capability(c: ResolvedCapability) -> bool {
    matches!(
        c,
        ResolvedCapability::WorkspaceFileRead
            | ResolvedCapability::MemoryRead
            | ResolvedCapability::GitRead
            | ResolvedCapability::ConfigurationRead
            | ResolvedCapability::ToolRegistryRead
            | ResolvedCapability::ToolEphemeralExposureWrite
            | ResolvedCapability::AgentTaskRead
            | ResolvedCapability::AgentTaskWrite
    )
}
/// Whether one capability asks for approval under a mode, ignoring the target.
///
/// This is the *capability* half of the decision and is deliberately narrow: the mode's
/// own default is decided once, by [`crate::mode_ceiling::permits`], and the target's
/// contribution (protected locations, the external-object rule) is decided by the caller.
/// It exists for the guided-profile rule below, which needs "is this capability a write
/// in this mode" without repeating the whole ceiling.
fn capability_asks(c: ResolvedCapability, mode: PermissionMode) -> bool {
    if read_capability(c) {
        return false;
    }
    if mode == PermissionMode::FullAccess
        && (crate::mode_ceiling::ordinary_network(c) || ordinary_external_file(c))
    {
        // FullAccess follows the existing policy for an ordinary external file: the user
        // confirmed that no per-directory grant has to be created first. A protected
        // resource still asks through the resource rule below, and the operations that
        // really change the machine or a remote are still forced once by `forced_once`;
        // this covers the ordinary file capabilities of FullAccess, including deleting or
        // renaming an object in a directory the user already granted.
        return false;
    }
    if mode == PermissionMode::ReadOnly && !crate::mode_ceiling::process(c) {
        return true;
    }
    // There is deliberately no WorkspaceReadWrite special case for `ExternalFileRead`. An
    // external object is never covered by the workspace default: WorkspaceReadWrite authorizes
    // reading *in the workspace*, and treating a declared read capability as authority over an
    // arbitrary reachable object is the substitution that would let a workspace-mode session
    // read outside its boundary with no approval at all. It asks, and the answer is a precise
    // approval for that object.
    !matches!(
        c,
        ResolvedCapability::WorkspaceFileWrite
            | ResolvedCapability::WorkspaceFileCreate
            | ResolvedCapability::WorkspaceFileDelete
            | ResolvedCapability::WorkspaceFileMove
            | ResolvedCapability::GitWrite
            | ResolvedCapability::MemoryWrite
            | ResolvedCapability::ConfigurationWrite
            | ResolvedCapability::ProcessExecute
            | ResolvedCapability::InteractiveTerminal
            | ResolvedCapability::BackgroundProcess
            | ResolvedCapability::PublicNetworkRead
            | ResolvedCapability::DownloadArtifact
            | ResolvedCapability::GitRemoteRead
            | ResolvedCapability::ExternalExtensionCall
    )
}

/// An ordinary external file operation the session mode can decide on its own.
///
/// Deleting is in this set for FullAccess for the same reason writing is: the confirmed
/// behaviour is that an operation inside a directory the user already granted — deleting,
/// overwriting and renaming included — is an ordinary write in that scope and does not ask
/// a second time. It is deliberately *not* in the WorkspaceReadWrite or ReadOnly defaults:
/// those modes never reach an ordinary external object without an approval, so the
/// approval flow decides, and the granted scope is what a later delete inherits.
fn ordinary_external_file(c: ResolvedCapability) -> bool {
    matches!(
        c,
        ResolvedCapability::ExternalFileRead
            | ResolvedCapability::ExternalFileWrite
            | ResolvedCapability::ExternalFileCreate
            | ResolvedCapability::ExternalFileDelete
    )
}

/// True for the capabilities that act on an object outside the workspace.
///
/// These are the ones a session grant can already authorize, so the policy only has to ask
/// about them when the operation reaches the object through an ordinary external path.
fn is_external_capability(c: ResolvedCapability) -> bool {
    matches!(
        c,
        ResolvedCapability::ExternalFileRead
            | ResolvedCapability::ExternalFileWrite
            | ResolvedCapability::ExternalFileCreate
            | ResolvedCapability::ExternalFileDelete
    )
}

/// The risk flags that are themselves a reason to ask.
///
/// A risk flag is not a synonym for "outside the workspace". The flags below name something a
/// directory scope cannot express — changing the machine, installing or elevating, destroying
/// a remote, acting inside the user's browser session — so they ask (and several are also
/// once-only) no matter which scope the operation is in.
///
/// `OutsideWorkspaceDelete` is deliberately **absent**. It describes where the object is, not
/// what the operation does to it: deleting inside a writable scope is an ordinary write in
/// that scope, and the confirmed rule is that it must not add a second question. Treating this
/// flag as a reason to ask is what made an ordinary granted-scope delete ask again. The write
/// still has to be inside a scope the user granted; that is what decides it, not the flag.
/// `OutsideWorkspaceWrite` is likewise a location fact rather than a new kind of authority.
fn operation_risk_asks_here(risk: OperationRiskFlag) -> bool {
    // A permanent deletion is decided with the mode, as a one-time question (`deletes_for_good`).
    !matches!(
        risk,
        OperationRiskFlag::OutsideWorkspaceWrite
            | OperationRiskFlag::OutsideWorkspaceDelete
            | OperationRiskFlag::PermanentFileDeletion
    )
}
