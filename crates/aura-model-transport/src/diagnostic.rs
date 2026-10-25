//! Bounded, structured evidence for a Provider response that Aura refused.
//!
//! The transport is the only layer that sees the compiled request, the adapter
//! identity and the normalized Provider output at the same time, so it is the
//! layer that can name a refused call exactly. Everything here is either
//! Aura-owned (identifiers, digest, adapter identity) or already bounded by the
//! domain check that produced it; no response body, argument value or
//! credential can enter this record.

use aura_domain_common::{ContentHash, TransportAttemptId};
use aura_model_domain::{
    ModelAdapterIdentity, ModelApiFormat, ModelExecutionSnapshot, ProviderRequestEvidence,
    ProviderToolCallBindingError,
};

/// Stable code of the refusal this diagnostic describes.
pub const PROVIDER_TOOL_CALL_BINDING_INVALID: &str = "provider_tool_call_binding_invalid";

/// A bounded detail line is a diagnostic, not a transcript.
const MAX_DETAIL_CHARS: usize = 256;
/// The offending wire name is already bounded by the domain check; this is the
/// second, independent bound on the copy that leaves the transport.
const MAX_NAME_CHARS: usize = 64;
/// How much of the frozen-list digest the one-line summary carries.
const SUMMARY_DIGEST_CHARS: usize = 16;

/// One refused Provider tool call, as bounded facts.
///
/// This is deliberately not a `NormalizedModelFailure`: that type is a persisted
/// contract, and a diagnostic that names the offending wire name belongs to the
/// process and the journal, not to the Provider-output schema.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderBindingViolationDiagnostic {
    /// Aura-owned code of the refusal.
    pub code: &'static str,
    /// Stable machine name of the exact domain violation.
    pub error_name: &'static str,
    /// Aura-generated explanation, bounded and single-line.
    pub detail: String,
    /// The Provider wire name that was refused, when the violation names one.
    pub refused_wire_name: Option<String>,
    /// The Provider item kind the adapter parsed the call from.
    pub provider_item_type: &'static str,
    /// Adapter identity that compiled the request.
    pub adapter: String,
    /// SHA-256 over the frozen wire names of the request that was frozen.
    pub frozen_tool_list_digest: String,
    /// How many tools the frozen request exposed.
    pub frozen_tool_count: u32,
    pub model_step_id: String,
    pub model_execution_snapshot_id: String,
    pub transport_attempt_id: String,
}

impl ProviderBindingViolationDiagnostic {
    /// Builds the diagnostic for a rejected Provider tool call.
    ///
    /// The frozen list is the one named by `evidence`, which is the compiled
    /// request the Provider answered — never a later or reloaded tool set.
    #[must_use]
    pub fn new(
        error: &ProviderToolCallBindingError,
        evidence: &ProviderRequestEvidence,
        snapshot: &ModelExecutionSnapshot,
        attempt_id: TransportAttemptId,
        provider_item_type: &'static str,
    ) -> Self {
        let mut wire_names = evidence
            .tool_name_bindings
            .iter()
            .map(|b| b.provider_wire_name.as_str())
            .collect::<Vec<_>>();
        wire_names.sort_unstable();
        let digest = ContentHash::sha256(wire_names.join("\n").as_bytes()).hex;
        Self {
            code: PROVIDER_TOOL_CALL_BINDING_INVALID,
            error_name: error.diagnostic_name(),
            detail: bounded_detail(&error.to_string()),
            refused_wire_name: error.offending_wire_name().map(bounded_wire_name),
            provider_item_type,
            adapter: adapter_identity(&evidence.adapter),
            frozen_tool_list_digest: digest,
            frozen_tool_count: u32::try_from(wire_names.len()).unwrap_or(u32::MAX),
            model_step_id: snapshot.model_step_id.to_string(),
            model_execution_snapshot_id: snapshot.id.to_string(),
            transport_attempt_id: attempt_id.to_string(),
        }
    }

    /// One bounded line for a stored error, a log or a UI detail view.
    ///
    /// The Aura-generated detail is kept verbatim so an existing reader that
    /// recognises `wire name <name> is not in request ...` keeps working; the
    /// identity fields follow it and add what that message never carried.
    #[must_use]
    pub fn summary(&self) -> String {
        let digest =
            self.frozen_tool_list_digest.chars().take(SUMMARY_DIGEST_CHARS).collect::<String>();
        format!(
            "{}: {} attempt={} adapter={} item={} frozen_tools=sha256:{} count={}",
            self.code,
            self.detail,
            self.transport_attempt_id,
            self.adapter,
            self.provider_item_type,
            digest,
            self.frozen_tool_count
        )
    }

    /// The same facts as a JSON object for the journal. Every value is already
    /// bounded, so the object cannot grow with a Provider response.
    #[must_use]
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "error_name": self.error_name,
            "detail": self.detail,
            "refused_wire_name": self.refused_wire_name,
            "provider_item_type": self.provider_item_type,
            "adapter": self.adapter,
            "frozen_tool_list_digest": self.frozen_tool_list_digest,
            "frozen_tool_count": self.frozen_tool_count,
            "model_step_id": self.model_step_id,
            "model_execution_snapshot_id": self.model_execution_snapshot_id,
            "transport_attempt_id": self.transport_attempt_id,
        })
    }
}

/// A stable adapter identity: the protocol family and the compilation contract
/// version, both Aura-owned. A model name or a base URL is not diagnostic here.
fn adapter_identity(adapter: &ModelAdapterIdentity) -> String {
    let format = match adapter.api_format {
        ModelApiFormat::OpenAiResponses => "openai_responses",
        ModelApiFormat::OpenAiChatCompletions => "openai_chat_completions",
        ModelApiFormat::AnthropicMessages => "anthropic_messages",
    };
    format!("{format}@{}.{}", adapter.contract_version.major, adapter.contract_version.minor)
}

/// Keeps a name-shaped token only. A Provider wire name is an identifier; any
/// other character is replaced, so no punctuation, quote or control byte from a
/// response can shape a stored diagnostic.
fn bounded_wire_name(value: &str) -> String {
    let mut bounded = String::new();
    for character in value.chars().take(MAX_NAME_CHARS) {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.' | ':') {
            bounded.push(character);
        } else {
            bounded.push('_');
        }
    }
    bounded
}

/// Bounds and flattens an Aura-generated explanation.
fn bounded_detail(value: &str) -> String {
    let mut bounded = String::new();
    for character in value.chars().take(MAX_DETAIL_CHARS) {
        if character.is_control() || character == '\n' || character == '\r' {
            bounded.push('_');
        } else {
            bounded.push(character);
        }
    }
    if value.chars().count() > MAX_DETAIL_CHARS {
        bounded.push('…');
    }
    bounded
}

#[cfg(test)]
mod tests {
    use aura_domain_common::jcs::CanonicalHashV1;
    use aura_domain_common::{
        ContentHash, ContentHashAlgorithm, ContextProjectionSnapshotId, EndpointId,
        ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
        Revision, ToolId, ToolRegistrySnapshotId, TransportAttemptId, UtcTimestamp,
    };
    use aura_model_domain::{
        ChatOutputTokenField, EndpointAuthScheme, ModelAdapterIdentity, ModelApiFormat,
        ModelExecutionSnapshot, ProviderRequestEvidence, ProviderRequestHeaderMetadata,
        ProviderRequestTargetMetadata, ProviderToolCallBindingError, ProviderToolNameBinding,
        ResolvedModelRequestOptions, TransportOperationContext,
    };
    use aura_permission_domain::NormalizedOrigin;
    use aura_tool_contract::{ToolInputSchemaV1, ToolSchemaSnapshotV1, ToolVersion};

    use super::ProviderBindingViolationDiagnostic;

    fn snapshot() -> ModelExecutionSnapshot {
        ModelExecutionSnapshot {
            id: ModelExecutionSnapshotId::new(),
            model_step_id: ModelStepId::new(),
            provider_id: ProviderId::new(),
            endpoint_id: EndpointId::new(),
            model_profile_id: ModelProfileId::new(),
            provider_display_name: "Provider".to_owned(),
            endpoint_display_name: "Endpoint".to_owned(),
            model_name: "fixture-model".to_owned(),
            readable_ref: "Provider / Endpoint / fixture-model".to_owned(),
            api_format: ModelApiFormat::OpenAiResponses,
            chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
            chat_instruction_role: None,
            auth_scheme: EndpointAuthScheme::None,
            base_url: "https://api.example.test/v1".to_owned(),
            resolved_request_url: "https://api.example.test/v1/responses".to_owned(),
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            endpoint_config_revision: 1,
            model_config_revision: 1,
            runtime_effective_window_tokens: 256_000,
            historical_snapshot_max_tokens: 1_024,
            request_options: ResolvedModelRequestOptions {
                effective_max_output_tokens: 512,
                temperature: None,
                top_p: None,
                reasoning_effort: None,
                reasoning_by_effort: false,
                image_input: false,
            },
            historical_tool_call_wire_bindings: Vec::new(),
            credential_id: None,
            credential_revision: None,
            credential_binding_fingerprint: None,
            context_snapshot_id: ContextProjectionSnapshotId::new(),
            prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
            endpoint_fingerprint: ContentHash::blake3(b"endpoint"),
            created_at: UtcTimestamp::now(),
        }
    }

    fn evidence(
        snapshot: &ModelExecutionSnapshot,
        canonical_names: &[&str],
    ) -> ProviderRequestEvidence {
        let registry_ref = aura_tool_contract::ToolRegistrySnapshotRef {
            tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
            registry_revision: Revision::INITIAL,
            content_hash: CanonicalHashV1::sha256(b"registry"),
        };
        ProviderRequestEvidence {
            body_sha256: ContentHash::new(ContentHashAlgorithm::Sha256, "11".repeat(32))
                .expect("sha256"),
            body_byte_length: 2,
            target: ProviderRequestTargetMetadata {
                method: "POST".to_owned(),
                origin: NormalizedOrigin {
                    scheme: "https".to_owned(),
                    host: "api.example.test".to_owned(),
                    port: 443,
                },
                path: "/v1/responses".to_owned(),
                query_parameter_names: Default::default(),
            },
            headers: Vec::<ProviderRequestHeaderMetadata>::new(),
            compatibility_removals: Default::default(),
            applied_output_token_field_alias: None,
            adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
            operation_context: TransportOperationContext::AgentModelAttempt {
                model_step_id: snapshot.model_step_id,
                model_execution_snapshot_id: snapshot.id,
                context_snapshot_id: snapshot.context_snapshot_id,
                prompt_bundle_snapshot_id: snapshot.prompt_bundle_snapshot_id,
            },
            tool_registry_snapshot_ref: Some(registry_ref.clone()),
            tool_name_bindings: canonical_names
                .iter()
                .enumerate()
                .map(|(index, canonical)| {
                    let tool_id = ToolId::from_bytes([u8::try_from(index + 1).unwrap_or(1); 16]);
                    let schema = ToolSchemaSnapshotV1::new(
                        tool_id,
                        ToolVersion::new(None, Revision::INITIAL).expect("version"),
                        (*canonical).parse().expect("canonical tool name"),
                        "Fixture tool".to_owned(),
                        ToolInputSchemaV1::new(serde_json::json!({
                            "type": "object",
                            "properties": {"value": {"type": "string"}},
                            "required": ["value"],
                            "additionalProperties": false
                        }))
                        .expect("tool input schema"),
                        true,
                    )
                    .expect("tool schema snapshot");
                    let (_, wrapped_hash) =
                        aura_model_domain::provider_arguments_envelope_schema_v3(
                            &schema.input_schema,
                        )
                        .expect("argument envelope");
                    ProviderToolNameBinding::from_snapshot(&registry_ref, &schema, wrapped_hash)
                })
                .collect(),
            historical_tool_call_wire_bindings: Vec::new(),
            canonical_body_artifact_id: None,
        }
    }

    #[test]
    fn names_the_refused_call_and_the_exact_frozen_request() {
        let snapshot = snapshot();
        let evidence = evidence(&snapshot, &["fixture.tool0", "fixture.tool1"]);
        let error = ProviderToolCallBindingError::UnknownProviderWireToolName {
            name: "_aura".to_owned(),
            request: format!("step={} snapshot={}", snapshot.model_step_id, snapshot.id),
        };
        let attempt_id = TransportAttemptId::new();
        let diagnostic = ProviderBindingViolationDiagnostic::new(
            &error,
            &evidence,
            &snapshot,
            attempt_id,
            "function_call",
        );
        assert_eq!(diagnostic.code, "provider_tool_call_binding_invalid");
        assert_eq!(diagnostic.error_name, "unknown_provider_wire_tool_name");
        assert_eq!(diagnostic.refused_wire_name.as_deref(), Some("_aura"));
        assert_eq!(diagnostic.provider_item_type, "function_call");
        assert_eq!(diagnostic.adapter, "openai_responses@1.0");
        assert_eq!(diagnostic.frozen_tool_count, 2);
        assert_eq!(diagnostic.frozen_tool_list_digest.len(), 64);
        assert_eq!(diagnostic.model_step_id, snapshot.model_step_id.to_string());
        assert_eq!(diagnostic.transport_attempt_id, attempt_id.to_string());
        let summary = diagnostic.summary();
        assert!(summary.starts_with("provider_tool_call_binding_invalid: wire name _aura"));
        assert!(summary.contains(&format!("attempt={attempt_id}")));
        assert!(summary.contains("adapter=openai_responses@1.0"));
        assert!(summary.contains(&format!("count={}", 2)));
        assert!(summary.len() < 512, "the bounded summary must stay small: {summary}");
        assert_eq!(diagnostic.to_json()["frozen_tool_count"], 2);
    }

    #[test]
    fn a_hostile_wire_name_cannot_carry_response_text() {
        let snapshot = snapshot();
        let evidence = evidence(&snapshot, &["fixture.tool0"]);
        let hostile = format!("{{\"body\":\"{}\"}}", "A".repeat(400));
        let error = ProviderToolCallBindingError::UnknownProviderWireToolName {
            name: hostile.clone(),
            request: "step=redacted snapshot=redacted".to_owned(),
        };
        let diagnostic = ProviderBindingViolationDiagnostic::new(
            &error,
            &evidence,
            &snapshot,
            TransportAttemptId::new(),
            "function_call",
        );
        let refused = diagnostic.refused_wire_name.clone().expect("refused name");
        assert!(refused.chars().count() <= 64);
        assert!(!refused.contains('"'));
        assert!(!refused.contains(','));
        assert!(diagnostic.summary().chars().count() <= 512);
    }
}
