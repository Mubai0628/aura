use super::*;

pub(super) fn authenticated_tool_mutation_identity_hash(
    method_name: &str,
    params: &Value,
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    channel: AuthenticatedConfigurationChannelIdentity,
) -> Result<ContentHash, IpcErrorEnvelope> {
    if (!TOOL_MUTATION_METHODS_V1.contains(&method_name)
        && !matches!(
            method_name,
            "tools.credential.create"
                | "tools.credential.rotate"
                | "tools.credential.clear"
                | "tools.credential.origins"
                | "tools.credential.recover"
        ))
        || !params.is_object()
    {
        return Err(envelope(
            "tool.mutation.method_invalid",
            ErrorCategory::Validation,
            "The authenticated Tool settings operation is invalid.",
            false,
            correlation_id,
        ));
    }
    let request_hash = aura_domain_common::jcs::canonical_sha256(params)
        .map_err(|error| internal_error(error, correlation_id))?;
    let evidence = AuthenticatedToolMutationEvidenceV1 {
        kind: "aura.p07.authenticated_tool_settings_mutation",
        version: 1,
        method_name,
        command_invocation_id,
        correlation_id,
        core_runtime_instance_id: channel.core_runtime_instance_id,
        authenticated_main_runtime_instance_id: channel.authenticated_main_runtime_instance_id,
        authenticated_channel_id: channel.authenticated_channel_id,
        source: "user_interface",
        request_hash: &request_hash,
    };
    let bytes = aura_domain_common::jcs::canonical_json_jcs(&evidence)
        .map_err(|error| internal_error(error, correlation_id))?;
    Ok(ContentHash::sha256(&bytes))
}

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct AuthenticatedToolMutationEvidenceV1<'a> {
    kind: &'static str,
    version: u8,
    method_name: &'a str,
    command_invocation_id: CommandInvocationId,
    correlation_id: CorrelationId,
    core_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    authenticated_main_runtime_instance_id: aura_domain_common::RuntimeInstanceId,
    authenticated_channel_id: aura_domain_common::IpcChannelId,
    source: &'static str,
    request_hash: &'a aura_domain_common::jcs::CanonicalSha256HashV1,
}
