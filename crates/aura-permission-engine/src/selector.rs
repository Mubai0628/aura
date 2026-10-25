use crate::policy::{PolicyInput, assess, authorize, required_for_backend};
use aura_execution_contract::{BackendDescriptor, BackendId};
use aura_security_contract::{
    AuthorizationDecision, BackendClass, PolicyAuthorization, PolicyDenyReason,
};

pub struct BackendCandidate {
    pub backend: BackendDescriptor,
    pub authorization: PolicyAuthorization,
}

/// Selection itself never starts a backend or rewrites an Operation. The
/// explicit ID is a request to preview a backend, not an approval to use it.
pub fn select(
    input: &PolicyInput<'_>,
    backends: &[BackendDescriptor],
    explicit: Option<&BackendId>,
    previous: Option<&AuthorizationDecision>,
    dispatch_started: bool,
) -> Result<Vec<BackendCandidate>, PolicyDenyReason> {
    if dispatch_started {
        return Err(PolicyDenyReason::DispatchAlreadyStarted);
    }
    let assessment = assess(input)?;
    let mut seen = std::collections::BTreeSet::new();
    if backends.iter().any(|b| !seen.insert(&b.id)) {
        return Err(PolicyDenyReason::BackendUnavailable);
    }
    let mut eligible = backends
        .iter()
        .filter(|b| {
            explicit.is_none_or(|id| id == &b.id)
                && (b.class != BackendClass::Native || explicit.is_some())
                && b.supports_operation(
                    &required_for_backend(&assessment, input.operation, b.class),
                    input.operation,
                )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut ordered = Vec::new();
    // Pareto fronts avoid inventing a numeric isolation level. A backend whose
    // guarantee set is a strict superset dominates one with fewer guarantees.
    while !eligible.is_empty() {
        let best = eligible
            .iter()
            .enumerate()
            .filter(|(_, b)| !eligible.iter().any(|other| dominates(other, b)))
            .min_by_key(|(_, b)| (preference(b.class), b.id.clone()))
            .map(|(i, _)| i)
            .ok_or(PolicyDenyReason::BackendUnavailable)?;
        let backend = eligible.remove(best);
        let authorization = authorize(input, &backend, previous);
        ordered.push(BackendCandidate { backend, authorization });
    }
    if ordered.is_empty() {
        return Err(PolicyDenyReason::BackendUnavailable);
    }
    Ok(ordered)
}
/// Strong offline includes direct-connection isolation for ranking only.
/// Authorization and dispatch still compare the original exact guarantee IDs;
/// a DirectNetworkBlockedV1 backend cannot consume an OfflineNetwork grant.
fn dominates(stronger: &BackendDescriptor, weaker: &BackendDescriptor) -> bool {
    use aura_security_contract::EnforcementGuarantee::{DirectNetworkBlockedV1, OfflineNetwork};
    let expanded = |backend: &BackendDescriptor| {
        let mut facts = backend.guarantees.clone();
        if facts.contains(&OfflineNetwork) {
            facts.insert(DirectNetworkBlockedV1);
        }
        facts
    };
    let a = expanded(stronger);
    let b = expanded(weaker);
    b.is_subset(&a)
        && stronger.known_limitations.is_subset(&weaker.known_limitations)
        && (a != b || stronger.known_limitations != weaker.known_limitations)
}
fn preference(class: BackendClass) -> u8 {
    match class {
        BackendClass::CoreBroker => 0,
        BackendClass::RestrictedUserOffline => 1,
        BackendClass::AppContainer => 2,
        BackendClass::Container => 3,
        BackendClass::RestrictedUserOnline => 4,
        BackendClass::Native => 5,
    }
}
