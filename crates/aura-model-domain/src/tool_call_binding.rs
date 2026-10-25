use std::collections::{BTreeMap, BTreeSet};

use aura_domain_common::{ContentHash, EndpointId, ModelExecutionSnapshotId};
use aura_domain_common::{ToolCallId, ToolId};
use aura_tool_contract::NormalizedToolCallV1;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    ModelApiFormat, ModelExecutionSnapshot, NormalizedModelOutput, ProviderRequestEvidence,
    ProviderToolCallId, ProviderWireToolCallId, TransportOperationContext,
    normalize_provider_tool_call_input,
};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderToolCallReuseDomain {
    pub endpoint_id: EndpointId,
    pub endpoint_security_fingerprint: ContentHash,
    pub api_format: ModelApiFormat,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProviderToolCallWireIdentity {
    ProviderOpaque {
        provider_tool_call_id: ProviderToolCallId,
        reuse_domain: ProviderToolCallReuseDomain,
    },
    AuraGenerated {
        provider_wire_tool_call_id: ProviderWireToolCallId,
    },
}

impl ProviderToolCallWireIdentity {
    #[must_use]
    pub fn wire_id(&self) -> &str {
        match self {
            Self::ProviderOpaque { provider_tool_call_id, .. } => provider_tool_call_id.as_str(),
            Self::AuraGenerated { provider_wire_tool_call_id } => {
                provider_wire_tool_call_id.as_str()
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderToolCallWireBinding {
    pub tool_call_id: ToolCallId,
    /// Null only for a call whose Provider name was absent from the frozen request.
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(with = "aura_domain_common::nullable::Required<ToolId>", required)]
    pub tool_id: Option<ToolId>,
    pub provider_wire_tool_name: String,
    pub wire_identity: ProviderToolCallWireIdentity,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderToolCallProtocolBindingRecord {
    pub model_execution_snapshot_id: ModelExecutionSnapshotId,
    pub tool_call_id: ToolCallId,
    /// Null only for a call whose Provider name was absent from the frozen request.
    #[serde(deserialize_with = "aura_domain_common::nullable::required")]
    #[schemars(
        with = "aura_domain_common::nullable::Required<aura_tool_contract::ToolDefinitionSnapshotRef>",
        required
    )]
    pub tool_definition: Option<aura_tool_contract::ToolDefinitionSnapshotRef>,
    pub provider_wire_tool_name: String,
    pub provider_tool_call_id: ProviderToolCallId,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ProviderToolCallBindingError {
    #[error("historical tool-call bindings must be strictly ordered by ToolCallId")]
    InvalidOrder,
    #[error("historical tool-call provider wire names must be non-blank")]
    InvalidToolName,
    #[error("historical tool-call wire identities must be unique within one request")]
    DuplicateWireIdentity,
    #[error("provider opaque tool-call identity does not belong to the current reuse domain")]
    ReuseDomainMismatch,
    #[error("Aura-generated tool-call identity does not match its ToolCallId")]
    AuraIdentityMismatch,
    #[error("wire name {name} is not in request {request}")]
    UnknownProviderWireToolName { name: String, request: String },
    #[error("provider output tool-call identities are not unique")]
    DuplicateOutputIdentity,
    #[error("provider request evidence is invalid")]
    InvalidRequestEvidence,
    #[error("provider request evidence does not belong to the model execution snapshot")]
    ExecutionIdentityMismatch,
    #[error("normalized model output does not belong to the model execution snapshot")]
    OutputIdentityMismatch,
    #[error("provider output tool-call ordinals are not unique")]
    DuplicateOutputOrdinal,
    #[error("a normalized provider tool call has no exact protocol binding")]
    MissingProtocolBinding,
}

fn bound_diagnostic(value: &str) -> String {
    let mut bounded = String::new();
    for character in value.chars().take(64) {
        if character.is_control() || character.is_whitespace() {
            bounded.push('_');
        } else {
            bounded.push(character);
        }
    }
    if value.chars().count() > 64 {
        bounded.push('…');
    }
    bounded
}

impl ProviderToolCallBindingError {
    /// Stable machine name for a bounded diagnostic. A display string is for a
    /// human; this name is what an operator or a UI can group by, and it never
    /// changes when a message is reworded.
    #[must_use]
    pub const fn diagnostic_name(&self) -> &'static str {
        match self {
            Self::InvalidOrder => "invalid_order",
            Self::InvalidToolName => "invalid_tool_name",
            Self::DuplicateWireIdentity => "duplicate_wire_identity",
            Self::ReuseDomainMismatch => "reuse_domain_mismatch",
            Self::AuraIdentityMismatch => "aura_identity_mismatch",
            Self::UnknownProviderWireToolName { .. } => "unknown_provider_wire_tool_name",
            Self::DuplicateOutputIdentity => "duplicate_output_identity",
            Self::InvalidRequestEvidence => "invalid_request_evidence",
            Self::ExecutionIdentityMismatch => "execution_identity_mismatch",
            Self::OutputIdentityMismatch => "output_identity_mismatch",
            Self::DuplicateOutputOrdinal => "duplicate_output_ordinal",
            Self::MissingProtocolBinding => "missing_protocol_binding",
        }
    }

    /// The offending Provider wire name, already bounded to 64 characters by
    /// [`Self::UnknownProviderWireToolName`] construction. It is reported only
    /// for the one failure where naming it is the point of the diagnostic; every
    /// other variant returns `None` so no other Provider text can reach a log.
    #[must_use]
    pub fn offending_wire_name(&self) -> Option<&str> {
        match self {
            Self::UnknownProviderWireToolName { name, .. } => Some(name.as_str()),
            _ => None,
        }
    }
}

pub fn derive_provider_tool_call_protocol_bindings(
    snapshot: &ModelExecutionSnapshot,
    evidence: &ProviderRequestEvidence,
    output: &NormalizedModelOutput,
) -> Result<Vec<ProviderToolCallProtocolBindingRecord>, ProviderToolCallBindingError> {
    let tool_names = evidence
        .tool_name_bindings
        .iter()
        .map(|binding| (binding.provider_wire_name.as_str(), binding.tool_definition.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut tool_call_ids = BTreeSet::new();
    let mut provider_ids = BTreeSet::new();
    let mut records = Vec::with_capacity(output.tool_calls.len());
    for tool_call in &output.tool_calls {
        if !tool_call_ids.insert(tool_call.tool_call_id)
            || !provider_ids.insert(tool_call.provider_call_id.as_str())
        {
            return Err(ProviderToolCallBindingError::DuplicateOutputIdentity);
        }
        if tool_call.tool_name.trim().is_empty() {
            return Err(ProviderToolCallBindingError::InvalidToolName);
        }
        let tool_definition = tool_names.get(tool_call.tool_name.as_str()).cloned();
        if tool_definition.is_none() && !evidence.allows_unknown_tool_names() {
            return Err(ProviderToolCallBindingError::UnknownProviderWireToolName {
                name: bound_diagnostic(&tool_call.tool_name),
                request: format!(
                    "step={} snapshot={}",
                    bound_diagnostic(&snapshot.model_step_id.to_string()),
                    bound_diagnostic(&snapshot.id.to_string())
                ),
            });
        }
        records.push(ProviderToolCallProtocolBindingRecord {
            model_execution_snapshot_id: snapshot.id,
            tool_call_id: tool_call.tool_call_id,
            tool_definition,
            provider_wire_tool_name: tool_call.tool_name.clone(),
            provider_tool_call_id: tool_call.provider_call_id.clone(),
        });
    }
    records.sort_by_key(|record| record.tool_call_id);
    Ok(records)
}

/// Converts one completed Provider response into the exact P05 -> P07 call input.
///
/// The conversion is deliberately independent of P08 batch state: it preserves the
/// Provider ordinal, resolves the Tool definition only through the request's frozen
/// Registry snapshot, and normalizes each call's arguments envelope without creating
/// a ToolResult or replacing any Core-owned identity.
pub fn derive_p07_normalized_tool_calls_v1(
    snapshot: &ModelExecutionSnapshot,
    evidence: &ProviderRequestEvidence,
    output: &NormalizedModelOutput,
) -> Result<Vec<NormalizedToolCallV1>, ProviderToolCallBindingError> {
    evidence.validate().map_err(|_| ProviderToolCallBindingError::InvalidRequestEvidence)?;
    let belongs_to_snapshot = matches!(
        &evidence.operation_context,
        TransportOperationContext::AgentModelAttempt {
            model_step_id,
            model_execution_snapshot_id,
            context_snapshot_id,
            prompt_bundle_snapshot_id,
        } if *model_step_id == snapshot.model_step_id
            && *model_execution_snapshot_id == snapshot.id
            && *context_snapshot_id == snapshot.context_snapshot_id
            && *prompt_bundle_snapshot_id == snapshot.prompt_bundle_snapshot_id
    ) && evidence.adapter.api_format == snapshot.api_format;
    if !belongs_to_snapshot {
        return Err(ProviderToolCallBindingError::ExecutionIdentityMismatch);
    }
    if output.model_step_id != snapshot.model_step_id {
        return Err(ProviderToolCallBindingError::OutputIdentityMismatch);
    }

    let protocol_bindings =
        derive_provider_tool_call_protocol_bindings(snapshot, evidence, output)?;
    // Each call is read in the envelope its own binding promised the Provider: stored steps
    // keep the envelope they were sent with, so recovery and history stay unchanged.
    let envelope_versions = evidence
        .tool_name_bindings
        .iter()
        .map(|binding| {
            (binding.provider_wire_name.as_str(), binding.provider_arguments_envelope_version)
        })
        .collect::<BTreeMap<_, _>>();
    let mut definitions = protocol_bindings
        .into_iter()
        .map(|binding| (binding.tool_call_id, binding.tool_definition))
        .collect::<BTreeMap<_, _>>();
    let mut ordinals = BTreeSet::new();
    let mut calls = Vec::with_capacity(output.tool_calls.len());
    for tool_call in &output.tool_calls {
        if !ordinals.insert(tool_call.position) {
            return Err(ProviderToolCallBindingError::DuplicateOutputOrdinal);
        }
        let definition_ref = definitions
            .remove(&tool_call.tool_call_id)
            .ok_or(ProviderToolCallBindingError::MissingProtocolBinding)?;
        let envelope_version = envelope_versions
            .get(tool_call.tool_name.as_str())
            .copied()
            .or_else(|| {
                definition_ref.is_none().then_some(crate::PROVIDER_ARGUMENTS_ENVELOPE_VERSION_V3)
            })
            .ok_or(ProviderToolCallBindingError::MissingProtocolBinding)?;
        let normalized_input =
            normalize_provider_tool_call_input(envelope_version, &tool_call.arguments_json);
        calls.push(NormalizedToolCallV1 {
            tool_call_id: tool_call.tool_call_id,
            ordinal: tool_call.position,
            definition_ref,
            input: normalized_input.input,
            provider_arguments_envelope_hash: normalized_input.provider_arguments_envelope_hash,
        });
    }
    if !definitions.is_empty() {
        return Err(ProviderToolCallBindingError::MissingProtocolBinding);
    }
    calls.sort_by_key(|call| call.ordinal);
    Ok(calls)
}

pub fn validate_provider_tool_call_wire_bindings(
    bindings: &[ProviderToolCallWireBinding],
    current_domain: &ProviderToolCallReuseDomain,
) -> Result<(), ProviderToolCallBindingError> {
    if !bindings.windows(2).all(|pair| pair[0].tool_call_id < pair[1].tool_call_id) {
        return Err(ProviderToolCallBindingError::InvalidOrder);
    }
    let mut wire_ids = BTreeSet::new();
    for binding in bindings {
        if binding.provider_wire_tool_name.trim().is_empty() {
            return Err(ProviderToolCallBindingError::InvalidToolName);
        }
        if !wire_ids.insert(binding.wire_identity.wire_id()) {
            return Err(ProviderToolCallBindingError::DuplicateWireIdentity);
        }
        match &binding.wire_identity {
            ProviderToolCallWireIdentity::ProviderOpaque { reuse_domain, .. }
                if reuse_domain != current_domain =>
            {
                return Err(ProviderToolCallBindingError::ReuseDomainMismatch);
            }
            ProviderToolCallWireIdentity::AuraGenerated { provider_wire_tool_call_id }
                if provider_wire_tool_call_id
                    != &ProviderWireToolCallId::from_tool_call_id(binding.tool_call_id) =>
            {
                return Err(ProviderToolCallBindingError::AuraIdentityMismatch);
            }
            ProviderToolCallWireIdentity::ProviderOpaque { .. }
            | ProviderToolCallWireIdentity::AuraGenerated { .. } => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use aura_domain_common::ToolCallId;
    use aura_domain_common::ToolId;
    use aura_domain_common::{ContentHash, EndpointId};

    use super::{
        ProviderToolCallBindingError, ProviderToolCallReuseDomain, ProviderToolCallWireBinding,
        ProviderToolCallWireIdentity, bound_diagnostic, validate_provider_tool_call_wire_bindings,
    };
    use crate::{ModelApiFormat, ProviderToolCallId, ProviderWireToolCallId};

    fn domain() -> ProviderToolCallReuseDomain {
        ProviderToolCallReuseDomain {
            endpoint_id: EndpointId::new(),
            endpoint_security_fingerprint: ContentHash::sha256(b"endpoint"),
            api_format: ModelApiFormat::OpenAiResponses,
        }
    }

    #[test]
    fn rejects_cross_domain_opaque_ids_and_mismatched_aura_ids() {
        let current = domain();
        let tool_call_id = ToolCallId::new();
        let opaque = ProviderToolCallWireBinding {
            tool_call_id,
            tool_id: Some(ToolId::new()),
            provider_wire_tool_name: "tool__01arz3ndektsv4rrffq69g5fav".to_owned(),
            wire_identity: ProviderToolCallWireIdentity::ProviderOpaque {
                provider_tool_call_id: ProviderToolCallId::new("call_exact").expect("provider ID"),
                reuse_domain: domain(),
            },
        };
        assert!(validate_provider_tool_call_wire_bindings(&[opaque], &current).is_err());

        let generated = ProviderToolCallWireBinding {
            tool_call_id,
            tool_id: Some(ToolId::new()),
            provider_wire_tool_name: "tool__01arz3ndektsv4rrffq69g5fav".to_owned(),
            wire_identity: ProviderToolCallWireIdentity::AuraGenerated {
                provider_wire_tool_call_id: ProviderWireToolCallId::from_tool_call_id(
                    ToolCallId::new(),
                ),
            },
        };
        assert!(validate_provider_tool_call_wire_bindings(&[generated], &current).is_err());
    }

    /// The diagnostic name is a stable machine name; the message may be reworded
    /// without breaking an operator's grouping or a UI's mapping.
    #[test]
    fn binding_errors_expose_a_stable_diagnostic_name() {
        let unknown = ProviderToolCallBindingError::UnknownProviderWireToolName {
            name: "_aura".to_owned(),
            request: "step=mst_exact snapshot=mes_exact".to_owned(),
        };
        assert_eq!(unknown.diagnostic_name(), "unknown_provider_wire_tool_name");
        assert_eq!(unknown.offending_wire_name(), Some("_aura"));
        assert!(unknown.to_string().contains("wire name _aura"));
        assert!(unknown.to_string().contains("step=mst_exact"));
        // Every other variant must not name a Provider value.
        for error in [
            ProviderToolCallBindingError::InvalidOrder,
            ProviderToolCallBindingError::MissingProtocolBinding,
        ] {
            assert_eq!(error.offending_wire_name(), None);
            assert!(!error.diagnostic_name().is_empty());
        }
    }

    /// The bounded copy keeps a hostile wire name short, single-line and free of
    /// control characters, so it can never carry a response body or a newline
    /// into a persisted diagnostic.
    #[test]
    fn the_reported_wire_name_stays_bounded_and_single_line() {
        let hostile = format!("bad\u{7}name\n{}secret-marker", "y".repeat(200));
        let bounded = bound_diagnostic(&hostile);
        assert_eq!(bounded.chars().count(), 65, "64 characters plus the truncation marker");
        assert!(!bounded.contains('\n'));
        assert!(!bounded.contains('\u{7}'));
        assert!(!bounded.contains("secret-marker"), "a body after the bound must not survive");
    }
}
