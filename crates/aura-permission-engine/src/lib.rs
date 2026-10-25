#![forbid(unsafe_code)]

mod browser;
pub mod dispatch_guard;
pub mod mode_ceiling;
pub mod network;
pub mod policy;
pub mod selector;

use aura_domain_common::{PermissionGrantId, UtcTimestamp};
use aura_permission_domain::{
    ApprovalRequestDraft, NativeSandboxMode, NetworkAccessRequirement, OperationSecurityPlan,
    PermissionDecision, PermissionDenyReason, PermissionEvaluationOutcomeV1, PermissionGrant,
    SecuritySource, TurnDenyRecord,
};
use aura_security_contract::TrustState;
use aura_security_contract::{
    ApprovalDecisionKind, InvocationActor, NetworkDataClass, OperationRiskFlag,
    PermissionAllowAuthorizationBasisV1, PermissionGrantLifetime, PermissionMode,
    PermissionSnapshot, ResolvedCapability,
};

pub trait PermissionEvaluator {
    fn evaluate(&self, input: &PermissionEvaluation<'_>) -> PermissionDecision;
}

#[derive(Clone, Debug)]
pub struct PermissionEvaluation<'a> {
    pub plan: &'a OperationSecurityPlan,
    pub snapshot: PermissionSnapshot,
    pub source_enabled: bool,
    pub trust_state: &'a TrustState,
    pub grants: &'a [PermissionGrant],
    pub turn_denies: &'a [TurnDenyRecord],
    pub available_sandbox_modes: &'a [NativeSandboxMode],
    pub network_control_available: bool,
    pub allow_unrestricted_inline_scripts_in_full_access: bool,
    pub now: UtcTimestamp,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultPermissionEvaluator;

impl PermissionEvaluator for DefaultPermissionEvaluator {
    fn evaluate(&self, input: &PermissionEvaluation<'_>) -> PermissionDecision {
        self.evaluate_with_authorization_basis(input).decision
    }
}

impl DefaultPermissionEvaluator {
    /// Evaluates the existing P04 decision while retaining the exact,
    /// type-safe Allow basis for the Core-owned W7 dispatch binding.
    #[must_use]
    pub fn evaluate_with_authorization_basis(
        &self,
        input: &PermissionEvaluation<'_>,
    ) -> PermissionEvaluationOutcomeV1 {
        let (decision, allow_authorization_basis) = evaluate_with_authorization_basis(input);
        PermissionEvaluationOutcomeV1 { decision, allow_authorization_basis }
    }
}

fn evaluate_with_authorization_basis(
    input: &PermissionEvaluation<'_>,
) -> (PermissionDecision, Option<PermissionAllowAuthorizationBasisV1>) {
    let plan = input.plan;

    // 1. Runtime identity is validated before any model-controlled field.
    if !identity_is_valid(plan, &input.snapshot) {
        return denied(PermissionDenyReason::IdentityInvalid);
    }
    if plan.identity.requires_model_purpose()
        && plan.purpose.as_deref().is_none_or(|purpose| purpose.trim().is_empty())
    {
        return denied(PermissionDenyReason::PurposeRequired);
    }

    // 2. Disabled and untrusted sources never reach capability evaluation.
    if !input.source_enabled {
        return denied(PermissionDenyReason::SourceDisabled);
    }
    if !plan
        .source
        .security_source_ref()
        .trust_is_effective(plan.source_fingerprint.as_ref(), input.trust_state)
    {
        return denied(PermissionDenyReason::SourceUntrusted);
    }

    // 3. Capabilities and scopes must already be resolved by the caller.
    if plan.capabilities.is_empty() {
        return denied(PermissionDenyReason::CapabilityUnresolved);
    }
    if plan.scopes.is_empty() {
        return denied(PermissionDenyReason::ScopeUnresolved);
    }
    if operation_requires_network_declaration(plan)
        && plan.network_access != NetworkAccessRequirement::Required
    {
        return denied(PermissionDenyReason::NetworkAccessDeclarationRequired);
    }

    // 4. Hard-deny invariants are evaluated before high-risk rules or Grants.
    if is_hard_denied(plan)
        || plan.capabilities.contains(&ResolvedCapability::OrdinaryNetwork)
        || plan.capabilities.iter().any(|c| {
            matches!(
                c,
                ResolvedCapability::MemoryRead
                    | ResolvedCapability::MemoryWrite
                    | ResolvedCapability::MemoryDelete
            )
        })
    {
        return denied(PermissionDenyReason::HardPolicy);
    }

    // The legacy P07 entry remains subject to the same Agent authority rules as V2.
    // The session mode is a default for *file* authority here too, so a file operation the
    // mode does not cover asks rather than being refused — that is how a ReadOnly session
    // approves a specified write. What stays refused is everything a file approval must not
    // become a route to: the user's configuration control plane, a child agent acting on
    // authority only its parent established, and **network**, which is its own capability
    // with its own stage. Legacy process execution also has no read-only filesystem
    // guarantee, so a ReadOnly Agent process call is refused rather than offered for
    // approval: there is no backend on this retired path that could honour a ReadOnly
    // guarantee it cannot enforce.
    let network_declared = operation_requires_network_declaration(plan)
        || plan.requested_sandbox == Some(NativeSandboxMode::ObservedNativeNetworked)
        || plan.capabilities.iter().any(|capability| {
            mode_ceiling::ordinary_network(*capability)
                || matches!(
                    capability,
                    ResolvedCapability::PublicNetworkRead
                        | ResolvedCapability::DownloadWindowsExecutable
                )
        });
    let mode_permits = mode_ceiling::permits(
        plan.identity.actor,
        input.snapshot.mode,
        &plan.capabilities,
        network_declared,
        false, // There is no Agent ControlledWeb owner on this retired path.
    );
    let read_only_agent_process =
        matches!(plan.identity.actor, InvocationActor::ParentAgent | InvocationActor::ChildAgent)
            && input.snapshot.mode == PermissionMode::ReadOnly
            && plan.capabilities.iter().copied().any(mode_ceiling::process);
    if !mode_permits
        && (read_only_agent_process
            || network_declared
            || mode_ceiling::refuses_outright(plan.identity.actor, &plan.capabilities))
    {
        return denied(PermissionDenyReason::ModeDenied);
    }
    // The mode's own verdict is applied **per capability** inside `capability_baseline`
    // rather than once here for the whole plan, because the baseline knows facts the mode
    // ceiling cannot see — a session authorized external file is allowed to read in ReadOnly,
    // and a blanket "outside the mode means ask" would make a grant the user already issued
    // unusable. A capability the mode does not reach and no prior authority covers asks
    // there, which is the case that makes a lower-mode approval possible.

    // 5. High-risk operations are constrained to an exact Once grant. This
    // classification happens before grant matching so a broader Session or
    // Workspace grant can never override the invariant.
    let forced_once = requires_forced_once_approval(plan)
        // The same permanent-deletion rule as `policy::deletes_for_good`.
        || input.snapshot.mode != PermissionMode::FullAccess
            && plan.risk_flags.contains(&OperationRiskFlag::PermanentFileDeletion);

    // 6. A current-turn deny prevents an unchanged request from reprompting.
    if denied_for_current_turn(plan, input.turn_denies) {
        return denied(PermissionDenyReason::DeniedForCurrentTurn);
    }

    // 7. Grants are exact, source-bound, risk-bounded, and lifetime-bound.
    // A matching Grant is an authorization basis, not proof that the
    // frozen Sandbox/Network execution path is currently available.
    let matching_grant = matching_grant_record(input)
        .filter(|grant| !forced_once || grant.lifetime == PermissionGrantLifetime::Once);
    if forced_once && matching_grant.is_none() {
        return asked(
            plan,
            &input.snapshot,
            vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny],
        );
    }

    // 8. PermissionMode supplies the baseline only after mandatory rules. An operation
    // the mode does not cover by default is not a refusal: the user may approve exactly
    // the capability, target and term it names, so it asks here.
    let basis = if let Some(grant) = matching_grant {
        PermissionAllowAuthorizationBasisV1::PermissionGrant {
            permission_grant_id: grant.id,
            lifetime: grant.lifetime,
        }
    } else {
        let baseline = baseline_decision(input);
        if baseline == BaselineDecision::Ask {
            return asked(plan, &input.snapshot, allowed_decisions(plan));
        }
        // The mode did not cover this plan, yet the baseline allowed it. That happens
        // exactly when the baseline recognised a fact the mode ceiling cannot see — a
        // session authorized external file, or a capability in `mode_asks`'s allowlist. Both
        // are real authority, so the operation proceeds; the plan is not silently widened
        // because the reason is recorded on the decision's basis below.
        PermissionAllowAuthorizationBasisV1::PermissionMode { mode: input.snapshot.mode }
    };

    // 9. A nominal allow is still denied when the requested execution path
    // cannot uphold the frozen sandbox/network plan. This applies equally
    // to PermissionMode and exact Grant Allow bases.
    if let Some(mode) = plan.requested_sandbox
        && !input.available_sandbox_modes.contains(&mode)
    {
        return denied(PermissionDenyReason::SandboxUnavailable);
    }
    if plan.network_plan.is_some() && !input.network_control_available {
        return denied(PermissionDenyReason::NetworkPlanUnavailable);
    }

    // 10. The immutable snapshot accompanies the final allow decision.
    (PermissionDecision::Allow { snapshot: input.snapshot.clone() }, Some(basis))
}

fn denied(
    reason: PermissionDenyReason,
) -> (PermissionDecision, Option<PermissionAllowAuthorizationBasisV1>) {
    (deny(reason), None)
}

fn asked(
    plan: &OperationSecurityPlan,
    snapshot: &PermissionSnapshot,
    allowed_decisions: Vec<ApprovalDecisionKind>,
) -> (PermissionDecision, Option<PermissionAllowAuthorizationBasisV1>) {
    (ask(plan, snapshot, allowed_decisions), None)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BaselineDecision {
    Allow,
    Ask,
}

fn identity_is_valid(plan: &OperationSecurityPlan, snapshot: &PermissionSnapshot) -> bool {
    if plan.identity.workspace_id != Some(snapshot.workspace_id)
        || plan.identity.session_id != Some(snapshot.session_id)
        || plan.identity.permission_snapshot_id.is_some_and(|id| id != snapshot.id)
    {
        return false;
    }
    match plan.identity.actor {
        InvocationActor::User | InvocationActor::SystemRuntime => {
            plan.identity.parent_agent_run_id.is_none()
        }
        InvocationActor::ParentAgent => {
            plan.identity.agent_run_id.is_some() && plan.identity.parent_agent_run_id.is_none()
        }
        InvocationActor::ChildAgent => {
            plan.identity.agent_run_id.is_some() && plan.identity.parent_agent_run_id.is_some()
        }
    }
}

fn is_hard_denied(plan: &OperationSecurityPlan) -> bool {
    plan.network_plan.as_ref().is_some_and(|network| {
        network.data_classes.contains(&NetworkDataClass::CredentialMaterial)
            || network.request_data.iter().any(|item| match item {
                aura_permission_domain::NetworkDataReference::Header { name, .. } => {
                    name.eq_ignore_ascii_case("proxy-authorization")
                }
                _ => false,
            })
    })
}

fn operation_requires_network_declaration(plan: &OperationSecurityPlan) -> bool {
    plan.network_plan.is_some()
        || plan.capabilities.iter().any(|capability| {
            matches!(
                capability,
                ResolvedCapability::GitRemoteRead
                    | ResolvedCapability::GitRemoteWrite
                    | ResolvedCapability::PublicNetworkRead
                    | ResolvedCapability::PrivateNetworkAccess
                    | ResolvedCapability::DataEgress
                    | ResolvedCapability::DownloadArtifact
                    | ResolvedCapability::DownloadWindowsExecutable
                    | ResolvedCapability::ExternalServiceRead
                    | ResolvedCapability::ExternalServiceMutation
                    | ResolvedCapability::ExternalServiceDestructiveMutation
                    | ResolvedCapability::BrowserRead
                    | ResolvedCapability::BrowserInteraction
                    | ResolvedCapability::BrowserSensitiveAction
            )
        })
}

fn requires_forced_once_approval(plan: &OperationSecurityPlan) -> bool {
    // An ordinary external delete is deliberately absent here: deleting, overwriting or
    // renaming inside a write scope the user already granted is covered by that scope's
    // approval, which may be a session or workspace term. See the V2 rule in
    // `crate::policy::forced_once` for the full reasoning; this legacy entry keeps the same
    // set so the two entries cannot disagree about what is reusable.
    const FORCED: &[OperationRiskFlag] = &[
        OperationRiskFlag::ExecutableArtifact,
        OperationRiskFlag::SoftwareInstallation,
        OperationRiskFlag::SystemConfigurationMutation,
        OperationRiskFlag::Elevation,
        OperationRiskFlag::DriverOrServiceMutation,
        OperationRiskFlag::DestructiveRemoteMutation,
        OperationRiskFlag::SensitiveBrowserAction,
        OperationRiskFlag::GitBareForcePush,
        OperationRiskFlag::GitSequencerSkipOrQuit,
    ];
    plan.risk_flags.iter().any(|risk| FORCED.contains(risk))
        || plan.capabilities.iter().any(|capability| {
            matches!(
                capability,
                ResolvedCapability::DownloadWindowsExecutable
                    | ResolvedCapability::ExternalServiceDestructiveMutation
                    | ResolvedCapability::GitDestructive
                    | ResolvedCapability::RegistryWrite
                    | ResolvedCapability::SystemEnvironmentWrite
                    | ResolvedCapability::SystemFileWrite
                    | ResolvedCapability::ServiceManagement
                    | ResolvedCapability::ScheduledTaskManagement
                    | ResolvedCapability::DriverManagement
                    | ResolvedCapability::WindowsSoftwareInstall
                    | ResolvedCapability::ElevatedOperation
            )
        })
}

fn denied_for_current_turn(plan: &OperationSecurityPlan, denies: &[TurnDenyRecord]) -> bool {
    plan.identity.turn_task_id.is_some_and(|turn_task_id| {
        denies.iter().any(|record| {
            record.turn_task_id == turn_task_id
                && record.source.security_source_ref() == plan.source.security_source_ref()
                && record.operation_fingerprint == plan.operation_fingerprint
        })
    })
}

#[must_use]
pub fn matching_grant(input: &PermissionEvaluation<'_>) -> Option<PermissionGrantId> {
    matching_grant_record(input).map(|grant| grant.id)
}

fn matching_grant_record<'a>(input: &'a PermissionEvaluation<'_>) -> Option<&'a PermissionGrant> {
    input.grants.iter().find(|grant| grant_matches(input, grant))
}

fn grant_matches(input: &PermissionEvaluation<'_>, grant: &PermissionGrant) -> bool {
    let plan = input.plan;
    if grant.workspace_id != input.snapshot.workspace_id
        || !input.snapshot.active_grant_ids.contains(&grant.id)
        || grant.revoked_at.is_some()
        || grant.consumed_at.is_some()
        || grant.expires_at.is_some_and(|expires| expires <= input.now)
        || grant.source.security_source_ref() != plan.source.security_source_ref()
        || grant.source_fingerprint != plan.source_fingerprint
        || !same_set(&grant.capabilities, &plan.capabilities)
        || !same_set(
            &grant.scopes.iter().map(|scope| scope.resource_descriptor()).collect::<Vec<_>>(),
            &plan.scopes.iter().map(|scope| scope.resource_descriptor()).collect::<Vec<_>>(),
        )
        || !plan.risk_flags.iter().all(|risk| grant.approved_risk_flags.contains(risk))
    {
        return false;
    }
    match grant.lifetime {
        PermissionGrantLifetime::Once => {
            grant.session_id == plan.identity.session_id
                && grant.operation_fingerprint.as_ref() == Some(&plan.operation_fingerprint)
        }
        PermissionGrantLifetime::TurnTask => {
            grant.session_id == plan.identity.session_id
                && grant.turn_task_id == plan.identity.turn_task_id
        }
        PermissionGrantLifetime::Session => grant.session_id == plan.identity.session_id,
        PermissionGrantLifetime::Workspace => true,
    }
}

fn same_set<T: PartialEq>(left: &[T], right: &[T]) -> bool {
    left.len() == right.len()
        && left.iter().all(|item| right.contains(item))
        && right.iter().all(|item| left.contains(item))
}

fn baseline_decision(input: &PermissionEvaluation<'_>) -> BaselineDecision {
    let plan = input.plan;
    if matches!(plan.source, SecuritySource::UserInterface)
        && !plan.risk_flags.contains(&OperationRiskFlag::DataLeavesDevice)
    {
        return BaselineDecision::Allow;
    }
    if plan.risk_flags.contains(&OperationRiskFlag::DataLeavesDevice)
        && input.snapshot.mode != PermissionMode::FullAccess
    {
        return BaselineDecision::Ask;
    }
    if plan.risk_flags.contains(&OperationRiskFlag::SensitivePathAccess) {
        return BaselineDecision::Ask;
    }
    if plan.risk_flags.contains(&OperationRiskFlag::PrivateOrLoopbackNetwork)
        && input.snapshot.mode != PermissionMode::FullAccess
    {
        return BaselineDecision::Ask;
    }
    if plan.risk_flags.contains(&OperationRiskFlag::InlineScript)
        && !(input.snapshot.mode == PermissionMode::FullAccess
            && input.allow_unrestricted_inline_scripts_in_full_access)
    {
        return BaselineDecision::Ask;
    }
    plan.capabilities.iter().fold(BaselineDecision::Allow, |decision, capability| {
        if decision == BaselineDecision::Ask
            || capability_baseline(*capability, input) == BaselineDecision::Ask
        {
            BaselineDecision::Ask
        } else {
            BaselineDecision::Allow
        }
    })
}

fn capability_baseline(
    capability: ResolvedCapability,
    input: &PermissionEvaluation<'_>,
) -> BaselineDecision {
    use BaselineDecision::{Allow, Ask};
    use PermissionMode::FullAccess;
    let mode = input.snapshot.mode;
    if mode == FullAccess && mode_ceiling::ordinary_network(capability) {
        return Allow;
    }
    // The mode's own verdict, asked per capability so the two sources of authority compose
    // instead of one overwriting the other. A capability the mode reaches on its own keeps
    // whatever the rules below say (which may still be Ask, for a protected target or a
    // risk flag). A capability the mode does **not** reach is an approval request unless the
    // rules below recognise authority the ceiling cannot see — today that is a session
    // authorized external file in ReadOnly. Concretely: the session mode stops being able to
    // *refuse* a file operation the user may approve, without any approval being assumed.
    let mode_reaches = mode_ceiling::permits(
        input.plan.identity.actor,
        mode,
        std::slice::from_ref(&capability),
        operation_requires_network_declaration(input.plan)
            || input.plan.requested_sandbox == Some(NativeSandboxMode::ObservedNativeNetworked),
        false,
    );
    let decided = capability_baseline_within_mode(capability, input);
    if mode_reaches {
        decided
    } else {
        match decided {
            // The baseline already wants an approval: keep it.
            Ask => Ask,
            // The baseline allowed it for a reason that outlives the mode's default — the
            // session grant the user issued for this file. Keep the allow.
            Allow if baseline_authority_outlives_mode(capability, input) => Allow,
            // Otherwise the mode does not reach it and no prior authority covers it: ask.
            Allow => Ask,
        }
    }
}

/// Whether the baseline's allow for this capability comes from authority the mode's default
/// does not bound, so it survives a mode that does not reach the capability on its own.
///
/// Today this is exactly one case, and it is deliberately narrow: a **session authorized
/// external file**, whose reference the user created by approving that file. The workspace
/// counterpart needs no such exception because no mode denies an ordinary workspace read.
fn baseline_authority_outlives_mode(
    capability: ResolvedCapability,
    input: &PermissionEvaluation<'_>,
) -> bool {
    capability == ResolvedCapability::ExternalFileRead
        && input.plan.scopes.iter().any(|scope| {
            matches!(scope, aura_permission_domain::ResolvedScope::SessionAuthorizedFile { .. })
        })
}

fn capability_baseline_within_mode(
    capability: ResolvedCapability,
    input: &PermissionEvaluation<'_>,
) -> BaselineDecision {
    use BaselineDecision::{Allow, Ask};
    use PermissionMode::{FullAccess, ReadOnly, WorkspaceReadWrite};
    use ResolvedCapability as C;
    let mode = input.snapshot.mode;
    if mode == FullAccess && mode_ceiling::ordinary_network(capability) {
        return Allow;
    }
    match capability {
        C::WorkspaceFileRead | C::GitRead | C::ConfigurationRead | C::ExternalFileRead
            if mode != ReadOnly =>
        {
            Allow
        }
        C::WorkspaceFileRead | C::GitRead | C::ConfigurationRead => Allow,
        C::ExternalFileRead
            if input.plan.scopes.iter().any(|scope| {
                matches!(scope, aura_permission_domain::ResolvedScope::SessionAuthorizedFile { .. })
            }) =>
        {
            Allow
        }
        C::ExternalFileRead => Ask,
        C::WorkspaceFileWrite
        | C::WorkspaceFileCreate
        | C::WorkspaceFileDelete
        | C::WorkspaceFileMove
        | C::GitWrite
        | C::ConfigurationWrite => {
            if mode == ReadOnly {
                Ask
            } else {
                Allow
            }
        }
        C::ExternalFileWrite | C::ExternalFileCreate => {
            if mode == FullAccess {
                Allow
            } else {
                Ask
            }
        }
        C::ExternalFileDelete => Ask,
        C::ProcessExecute | C::InteractiveTerminal | C::BackgroundProcess => match mode {
            ReadOnly => Ask,
            WorkspaceReadWrite => {
                if matches!(
                    input.plan.requested_sandbox,
                    Some(
                        NativeSandboxMode::CompatibilityAppContainer
                            | NativeSandboxMode::ObservedNativeNetworked
                    )
                ) {
                    Allow
                } else {
                    Ask
                }
            }
            FullAccess => Allow,
        },
        C::GitRemoteRead | C::PublicNetworkRead | C::DownloadArtifact => {
            if mode == ReadOnly {
                Ask
            } else {
                Allow
            }
        }
        C::OrdinaryNetwork
        | C::PrivateNetworkAccess
        | C::DataEgress
        | C::DownloadWindowsExecutable => Ask,
        C::GitRemoteWrite | C::ExternalServiceMutation | C::BrowserInteraction => {
            if mode == FullAccess { Allow } else { Ask }
        }
        C::ExternalServiceRead | C::BrowserRead => {
            if mode == ReadOnly {
                Ask
            } else {
                Allow
            }
        }
        C::GitDestructive
        | C::ExternalServiceDestructiveMutation
        | C::BrowserSensitiveAction
        | C::RegistryWrite
        | C::SystemEnvironmentWrite
        | C::SystemFileWrite
        | C::ServiceManagement
        | C::ScheduledTaskManagement
        | C::DriverManagement
        | C::WindowsSoftwareInstall
        | C::ElevatedOperation => Ask,
        C::ToolRegistryRead
        | C::ToolEphemeralExposureWrite
        | C::AgentTaskRead
        | C::AgentTaskWrite => Allow,
        C::MemoryRead | C::MemoryWrite | C::MemoryDelete => Ask,
        C::ExternalExtensionCall => {
            if mode == ReadOnly {
                Ask
            } else {
                Allow
            }
        }
    }
}

fn allowed_decisions(plan: &OperationSecurityPlan) -> Vec<ApprovalDecisionKind> {
    if matches!(plan.source, SecuritySource::McpServer { .. } | SecuritySource::Plugin { .. }) {
        return vec![ApprovalDecisionKind::AllowForCurrentTurn, ApprovalDecisionKind::Deny];
    }
    vec![
        ApprovalDecisionKind::AllowOnce,
        ApprovalDecisionKind::AllowSession,
        ApprovalDecisionKind::AllowWorkspace,
        ApprovalDecisionKind::Deny,
    ]
}

fn ask(
    plan: &OperationSecurityPlan,
    snapshot: &PermissionSnapshot,
    allowed_decisions: Vec<ApprovalDecisionKind>,
) -> PermissionDecision {
    PermissionDecision::Ask {
        request: Box::new(ApprovalRequestDraft {
            identity: plan.identity.clone(),
            operation_ids: vec![plan.operation_id],
            operation_fingerprints: vec![plan.operation_fingerprint.clone()],
            title: "Permission required".to_owned(),
            model_purpose: plan.purpose.clone(),
            actual_operation_summary: summarize_operation(plan),
            source_summary: plan.source.stable_key(),
            source: plan.source.clone(),
            source_fingerprint: plan.source_fingerprint.clone(),
            capabilities: plan.capabilities.clone(),
            scopes: plan.scopes.clone(),
            risk_flags: plan.risk_flags.clone(),
            allowed_decisions,
            permission_snapshot_id: snapshot.id,
            release_descriptor: None,
        }),
    }
}

fn summarize_operation(plan: &OperationSecurityPlan) -> String {
    format!("{} capabilities across {} resolved scopes", plan.capabilities.len(), plan.scopes.len())
}

const fn deny(reason: PermissionDenyReason) -> PermissionDecision {
    PermissionDecision::Deny { reason }
}
