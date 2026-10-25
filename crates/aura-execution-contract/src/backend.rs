use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{Revision, RuntimeInstanceId};
use aura_security_contract::{
    BackendClass, BackendRequirement, EnforcementGuarantee, ResolvedCapability,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Registration key, not an allocated persistent entity ID.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize)]
#[serde(transparent)]
pub struct BackendId(pub String);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(tag = "readiness", rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendReadiness {
    Ready,
    Unavailable { reason: String },
    SetupRequired { reason: String },
    RepairRequired { reason: String },
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, JsonSchema, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum BackendKnownLimitation {
    /// Accepted by the owner: caller-controlled names and custom DNS servers
    /// may be serviced by Dnscache. Does not accept unrelated service brokers.
    DnscacheServiceMediatedDns,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BackendDescriptor {
    pub known_limitations: BTreeSet<BackendKnownLimitation>,
    pub compatibility: BTreeSet<crate::ExecutionCompatibility>,
    pub id: BackendId,
    pub revision: Revision,
    pub runtime_instance_id: RuntimeInstanceId,
    pub class: BackendClass,
    pub capabilities: BTreeSet<ResolvedCapability>,
    pub guarantees: BTreeSet<EnforcementGuarantee>,
    pub readiness: BackendReadiness,
    /// Identifies the backend's exact supported action protocol.
    pub protocol_fingerprint: CanonicalSha256HashV1,
}

impl BackendDescriptor {
    #[must_use]
    pub fn supports_operation(
        &self,
        required: &BackendRequirement,
        operation: &crate::SecuredOperation,
    ) -> bool {
        self.supports(required, &operation.content.capabilities)
            && operation.content.compatibility.is_subset(&self.compatibility)
    }

    /// The same comparison as [`Self::supports_operation`] without the readiness
    /// requirement: whether this backend is the *right kind* of backend for the
    /// call, if it were healthy.
    ///
    /// The two questions are asked at different moments and must not be fused.
    /// "Could this class of backend run the call at all" is a question about the
    /// call and the backend's declared capabilities; it has an answer while the
    /// installation is being repaired. "Is this backend usable right now" is a
    /// question about the installation. A caller that only had the fused predicate
    /// would read a `RepairRequired` backend as "this backend cannot run the call",
    /// which is how an identity refusal previously turned into a general backend
    /// unavailability and then into a broader fallback.
    ///
    /// Readiness still gates dispatch: [`Self::supports_operation`] keeps requiring
    /// `Ready`, and every dispatch-time check uses that one.
    #[must_use]
    pub fn supports_capabilities(
        &self,
        required: &BackendRequirement,
        operation: &crate::SecuredOperation,
    ) -> bool {
        self.supports_declared(required, &operation.content.capabilities)
            && operation.content.compatibility.is_subset(&self.compatibility)
    }

    #[must_use]
    pub fn supports(
        &self,
        required: &BackendRequirement,
        capabilities: &[ResolvedCapability],
    ) -> bool {
        self.supports_declared(required, capabilities) && self.readiness == BackendReadiness::Ready
    }

    /// Declared capability, guarantee and class compatibility. No readiness.
    #[must_use]
    pub fn supports_declared(
        &self,
        required: &BackendRequirement,
        capabilities: &[ResolvedCapability],
    ) -> bool {
        !self.id.0.trim().is_empty()
            && self.revision.value() > 0
            && (!self.guarantees.contains(&EnforcementGuarantee::DirectNetworkBlockedV1)
                || (self.class == BackendClass::RestrictedUserOffline
                    && self
                        .known_limitations
                        .contains(&BackendKnownLimitation::DnscacheServiceMediatedDns)
                    && !self.guarantees.contains(&EnforcementGuarantee::OfflineNetwork)))
            && required.allowed_classes.contains(&self.class)
            && required.guarantees.is_subset(&self.guarantees)
            && capabilities.iter().all(|c| self.capabilities.contains(c))
    }
}
