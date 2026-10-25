use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{ArtifactId, ToolExecutionAttemptId, ToolWireVersionV1};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectStateV1 {
    NotStarted,
    NoEffect,
    FullyApplied,
    PartiallyApplied,
    Compensated,
    Unknown,
    /// Generic execution does not certify arbitrary script effects. This is
    /// neither a verified no-effect result nor missing execution information.
    Unassessed,
}

#[derive(
    Clone, Copy, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectEvidenceProducerV1 {
    CoreTransaction,
    StrictSandbox,
    WorkspaceJournal,
    TrustedNetworkAdapter,
    Reconciliation,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "evidence_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttemptSideEffectProofV1 {
    NoEffect {
        evidence_ref: ArtifactId,
    },
    FullyApplied {
        receipt_ref: ArtifactId,
        postcondition_ref: ArtifactId,
    },
    PartiallyApplied {
        receipt_ref: ArtifactId,
    },
    Compensated {
        original_effect_ref: ArtifactId,
        compensation_ref: ArtifactId,
        baseline_verification_ref: ArtifactId,
    },
    Unknown {
        evidence_ref: ArtifactId,
    },
}

impl AttemptSideEffectProofV1 {
    #[must_use]
    pub const fn state(&self) -> SideEffectStateV1 {
        match self {
            Self::NoEffect { .. } => SideEffectStateV1::NoEffect,
            Self::FullyApplied { .. } => SideEffectStateV1::FullyApplied,
            Self::PartiallyApplied { .. } => SideEffectStateV1::PartiallyApplied,
            Self::Compensated { .. } => SideEffectStateV1::Compensated,
            Self::Unknown { .. } => SideEffectStateV1::Unknown,
        }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum AttemptSideEffectEvidenceErrorV1 {
    #[error("attempt side-effect evidence hash is inconsistent")]
    HashMismatch,
}

/// Core-verified evidence for one exact Runtime attempt. The Runtime-facing
/// candidate contract never carries this type; only trusted Core adapters may
/// create it after checking the declared effect scope and receipt facts.
#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedAttemptSideEffectEvidenceV1 {
    evidence_policy_version: ToolWireVersionV1,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    effect_scope_hash: CanonicalSha256HashV1,
    producer: SideEffectEvidenceProducerV1,
    proof: AttemptSideEffectProofV1,
    evidence_hash: CanonicalSha256HashV1,
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct AttemptSideEffectEvidenceHashMaterialV1<'a> {
    evidence_policy_version: ToolWireVersionV1,
    tool_execution_attempt_id: ToolExecutionAttemptId,
    effect_scope_hash: &'a CanonicalSha256HashV1,
    producer: SideEffectEvidenceProducerV1,
    proof: &'a AttemptSideEffectProofV1,
}

impl VerifiedAttemptSideEffectEvidenceV1 {
    pub fn from_core_evidence(
        tool_execution_attempt_id: ToolExecutionAttemptId,
        effect_scope_hash: CanonicalSha256HashV1,
        producer: SideEffectEvidenceProducerV1,
        proof: AttemptSideEffectProofV1,
    ) -> Result<Self, AttemptSideEffectEvidenceErrorV1> {
        let mut value = Self {
            evidence_policy_version: ToolWireVersionV1,
            tool_execution_attempt_id,
            effect_scope_hash,
            producer,
            proof,
            evidence_hash: CanonicalSha256HashV1::sha256(&[]),
        };
        value.evidence_hash = value.compute_hash()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), AttemptSideEffectEvidenceErrorV1> {
        (self.compute_hash()? == self.evidence_hash)
            .then_some(())
            .ok_or(AttemptSideEffectEvidenceErrorV1::HashMismatch)
    }

    #[must_use]
    pub const fn tool_execution_attempt_id(&self) -> ToolExecutionAttemptId {
        self.tool_execution_attempt_id
    }

    #[must_use]
    pub const fn state(&self) -> SideEffectStateV1 {
        self.proof.state()
    }

    #[must_use]
    pub const fn producer(&self) -> SideEffectEvidenceProducerV1 {
        self.producer
    }

    #[must_use]
    pub const fn proof(&self) -> &AttemptSideEffectProofV1 {
        &self.proof
    }

    #[must_use]
    pub const fn effect_scope_hash(&self) -> &CanonicalSha256HashV1 {
        &self.effect_scope_hash
    }

    #[must_use]
    pub const fn evidence_hash(&self) -> &CanonicalSha256HashV1 {
        &self.evidence_hash
    }

    fn compute_hash(&self) -> Result<CanonicalSha256HashV1, AttemptSideEffectEvidenceErrorV1> {
        aura_domain_common::jcs::canonical_sha256(&AttemptSideEffectEvidenceHashMaterialV1 {
            evidence_policy_version: self.evidence_policy_version,
            tool_execution_attempt_id: self.tool_execution_attempt_id,
            effect_scope_hash: &self.effect_scope_hash,
            producer: self.producer,
            proof: &self.proof,
        })
        .map_err(|_| AttemptSideEffectEvidenceErrorV1::HashMismatch)
    }
}

impl<'de> Deserialize<'de> for VerifiedAttemptSideEffectEvidenceV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            evidence_policy_version: ToolWireVersionV1,
            tool_execution_attempt_id: ToolExecutionAttemptId,
            effect_scope_hash: CanonicalSha256HashV1,
            producer: SideEffectEvidenceProducerV1,
            proof: AttemptSideEffectProofV1,
            evidence_hash: CanonicalSha256HashV1,
        }
        let wire = Wire::deserialize(deserializer)?;
        let value = Self {
            evidence_policy_version: wire.evidence_policy_version,
            tool_execution_attempt_id: wire.tool_execution_attempt_id,
            effect_scope_hash: wire.effect_scope_hash,
            producer: wire.producer,
            proof: wire.proof,
            evidence_hash: wire.evidence_hash,
        };
        value.validate().map_err(D::Error::custom)?;
        Ok(value)
    }
}
