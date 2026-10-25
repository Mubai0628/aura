// Only the isolated evaluation model configuration is edited. No real Provider config is read.
export async function setup(page, config) {
  return page.evaluate(async (c) => {
    const api = globalThis.window.aura.v1;
    const ok = (r) => {
      if (!r.ok) throw new Error(r.error.code);
      return r.value;
    };
    const workspace = ok(
      await api.createWorkspace({
        display_name: 'Isolated coding evaluation',
        selected_root: c.workspace,
      }),
    ).workspace;
    const session = ok(
      await api.createSession({ workspace_id: workspace.workspace_id, title: 'Evaluation' }),
    );
    const before = ok(await api.getSessionPermissionState({ session_id: session.session_id }));
    ok(
      await api.setSessionPermissionMode({
        session_id: session.session_id,
        expected_revision: before.revision,
        mode: c.mode,
      }),
    );
    const settings = ok(await api.getModelSettings());
    const provider = ok(
      await api.createModelProvider({
        expected_revision: settings.models_revision,
        input: {
          display_name: 'Metered local evaluation',
          source: { type: 'custom', catalog_provider_id: null },
        },
      }),
    );
    const endpoint = ok(
      await api.createModelEndpoint({
        expected_revision: provider.revision,
        provider_id: provider.value.id,
        display_name: 'Local meter',
        api_format: c.request_settings.api_format,
        base_url: c.provider_url,
        discovery_mode: { type: 'disabled' },
        chat_output_token_field: c.request_settings.chat_output_token_field,
        chat_instruction_role: c.request_settings.chat_instruction_role,
        auth_scheme: 'none',
        openai_organization_id: c.request_settings.openai_organization_id,
        openai_project_id: c.request_settings.openai_project_id,
        anthropic_workspace_id: null,
        timeouts: c.timeouts,
        max_parallel_model_requests: c.request_settings.max_parallel_model_requests,
      }),
    );
    const profile = ok(
      await api.createModelProfile({
        expected_revision: endpoint.revision,
        input: {
          endpoint_id: endpoint.value.id,
          provider_model_name: c.model,
          display_name: 'Metered coding fixture',
          notes: null,
          context_window_tokens: c.request_settings.context_window_tokens,
        },
      }),
    );
    ok(
      await api.updateModelProfile({
        expected_revision: profile.revision,
        model_profile_id: profile.value.id,
        display_name: 'Metered coding fixture',
        notes: null,
        enabled: true,
        request_defaults: c.request_settings.request_defaults,
        historical_snapshot_max_tokens: c.request_settings.historical_snapshot_max_tokens,
      }),
    );
    ok(
      await api.setSessionModelBinding({
        session_id: session.session_id,
        expected_revision: '0',
        model_profile_id: profile.value.id,
      }),
    );
    ok(
      await api.submitAgentMessage({
        session_id: session.session_id,
        destination: { lane: 'next_turn' },
        text: c.prompt,
      }),
    );
    return { session_id: session.session_id, workspace_id: workspace.workspace_id };
  }, config);
}
