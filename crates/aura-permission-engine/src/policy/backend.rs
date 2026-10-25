//! Backend enforcement floors remain separate from defaults and explicit Native exceptions.
use super::*;

pub fn required_for_backend(
    assessment: &PolicyAssessment,
    op: &SecuredOperation,
    class: BackendClass,
) -> BackendRequirement {
    let mut required = assessment.requirement.clone();
    if class != BackendClass::Native {
        required.guarantees.insert(EnforcementGuarantee::AuthorizedWritesOnly);
        if matches!(
            class,
            BackendClass::RestrictedUserOffline
                | BackendClass::RestrictedUserOnline
                | BackendClass::AppContainer
                | BackendClass::Container
        ) {
            required.guarantees.insert(EnforcementGuarantee::IdentityIsolation);
            if op.content.network == OperationNetwork::None {
                required.guarantees.insert(if class == BackendClass::RestrictedUserOffline {
                    EnforcementGuarantee::DirectNetworkBlockedV1
                } else {
                    EnforcementGuarantee::OfflineNetwork
                });
            }
        }
    }
    required
}
