use super::*;

#[test]
fn authenticated_main_gate_covers_every_workspace_mutation_and_no_lifecycle_command() {
    for method_name in [
        method::BUFFER_SAVE,
        method::FS_WRITE_FILE,
        method::FS_APPLY_PATCH,
        method::FS_CREATE_DIRECTORY,
        method::FS_COPY,
        method::FS_MOVE,
        method::FS_DELETE,
        method::FS_APPLY_CHANGESET,
    ] {
        assert!(
            P04Services::gates_authenticated_main_workspace_mutation(method_name),
            "missing P04 gate for {method_name}"
        );
    }
    for method_name in [
        method::WORKSPACE_CREATE,
        method::WORKSPACE_RESTORE,
        method::WORKSPACE_LIFECYCLE_BEGIN,
        method::WORKSPACE_LIFECYCLE_ACK,
        method::BUFFER_OPEN,
        method::BUFFER_EDIT,
        method::FS_READ_TEXT,
    ] {
        assert!(
            !P04Services::gates_authenticated_main_workspace_mutation(method_name),
            "P04 mutation gate must not change the confirmed P03 lifecycle/read boundary: {method_name}"
        );
    }
}

#[test]
fn authenticated_tool_mutation_authority_covers_only_the_closed_eight_methods() {
    let command_invocation_id = CommandInvocationId::new();
    let correlation_id = CorrelationId::new();
    let channel = AuthenticatedConfigurationChannelIdentity {
        core_runtime_instance_id: aura_domain_common::RuntimeInstanceId::new(),
        authenticated_main_runtime_instance_id: aura_domain_common::RuntimeInstanceId::new(),
        authenticated_channel_id: aura_domain_common::IpcChannelId::new(),
    };
    let params = serde_json::json!({ "expected_revision": 1 });
    let mut hashes = Vec::new();
    for method_name in TOOL_MUTATION_METHODS_V1 {
        hashes.push(
            authenticated_tool_mutation_identity_hash(
                method_name,
                &params,
                command_invocation_id,
                correlation_id,
                channel,
            )
            .unwrap_or_else(|error| panic!("missing P04 authority for {method_name}: {error:?}")),
        );
    }
    // Retired the Draft, package and operation-cancel commands with their routes.
    assert_eq!(hashes.len(), 8);
    assert_eq!(
        authenticated_tool_mutation_identity_hash(
            TOOL_MUTATION_METHODS_V1[0],
            &params,
            command_invocation_id,
            correlation_id,
            channel,
        )
        .expect("the same authenticated command remains stable"),
        hashes[0]
    );
    assert_ne!(hashes[0], hashes[1]);

    for (method_name, invalid_params) in [
        ("tools.activity.page", serde_json::json!({})),
        (TOOL_MUTATION_METHODS_V1[0], serde_json::json!([])),
    ] {
        let error = authenticated_tool_mutation_identity_hash(
            method_name,
            &invalid_params,
            command_invocation_id,
            correlation_id,
            channel,
        )
        .expect_err("P04 must reject anything outside the exact authenticated mutation set");
        assert_eq!(error.code.as_str(), "tool.mutation.method_invalid");
    }
}
