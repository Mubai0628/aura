import type { Page as ElectronPage } from '@playwright/test';

export async function armReadOnlyWrite(
  page: ElectronPage,
  workspaceRoot: string,
  providerUrl: string,
  message: { title: string; text: string },
): Promise<Record<string, unknown>> {
  return page.evaluate(
    async ({ selectedRoot, url, title, text }) => {
      const api = window.aura.v1;
      const workspace = await api.createWorkspace({
        display_name: title,
        selected_root: selectedRoot,
      });
      if (!workspace.ok) return { error: `workspace: ${JSON.stringify(workspace)}` };
      const workspaceId = workspace.value.workspace.workspace_id;
      const session = await api.createSession({ workspace_id: workspaceId, title: '' });
      if (!session.ok) return { error: `session: ${JSON.stringify(session)}` };
      const sessionId = session.value.session_id;
      // The setter is revision-guarded, so the current revision is read first: that guard is the
      // real one, and a test that bypassed it would not be exercising the product path.
      const before = await api.getSessionPermissionState({ session_id: sessionId });
      if (!before.ok) return { error: `permission state: ${JSON.stringify(before)}` };
      const mode = await api.setSessionPermissionMode({
        session_id: sessionId,
        expected_revision: before.value.revision,
        mode: 'read_only',
      });
      if (!mode.ok) return { error: `mode: ${JSON.stringify(mode)}` };

      const settings = await api.getModelSettings();
      if (!settings.ok) return { error: 'model settings unavailable' };
      const provider = await api.createModelProvider({
        expected_revision: settings.value.models_revision,
        input: {
          display_name: 'S06 isolated model',
          source: { type: 'custom', catalog_provider_id: null },
        },
      });
      if (!provider.ok) return { error: `provider: ${JSON.stringify(provider)}` };
      const endpoint = await api.createModelEndpoint({
        expected_revision: provider.value.revision,
        provider_id: provider.value.value.id,
        display_name: 'S06 local endpoint',
        api_format: 'open_ai_chat_completions',
        base_url: url,
        discovery_mode: { type: 'disabled' },
        chat_output_token_field: 'max_tokens',
        chat_instruction_role: 'system',
        auth_scheme: 'none',
        openai_organization_id: null,
        openai_project_id: null,
        anthropic_workspace_id: null,
        timeouts: {
          connect_ms: 5000,
          first_event_ms: 10_000,
          stream_idle_ms: 10_000,
          total_ms: 60_000,
        },
        max_parallel_model_requests: 2,
      });
      if (!endpoint.ok) return { error: `endpoint: ${JSON.stringify(endpoint)}` };
      const profile = await api.createModelProfile({
        expected_revision: endpoint.value.revision,
        input: {
          endpoint_id: endpoint.value.value.id,
          provider_model_name: 'fixture',
          display_name: 'S06 fixture model',
          notes: null,
          context_window_tokens: 256000,
        },
      });
      if (!profile.ok) return { error: `profile: ${JSON.stringify(profile)}` };
      const enabled = await api.updateModelProfile({
        expected_revision: profile.value.revision,
        model_profile_id: profile.value.value.id,
        display_name: 'S06 fixture model',
        notes: null,
        enabled: true,
        request_defaults: {
          max_output_tokens: 4096,
          temperature: null,
          top_p: null,
          chat_output_token_field: null,
          chat_instruction_role: null,
        },
        historical_snapshot_max_tokens: 1024,
      });
      if (!enabled.ok) return { error: `profile enable: ${JSON.stringify(enabled)}` };
      const profileId = profile.value.value.id;
      // A brand-new session has never been bound, so its binding revision is 0.
      const binding = await api.setSessionModelBinding({
        session_id: sessionId,
        expected_revision: '0',
        model_profile_id: profileId,
      });
      if (!binding.ok) return { error: `binding: ${JSON.stringify(binding)}` };
      const submitted = await api.submitAgentMessage({
        session_id: sessionId,
        destination: { lane: 'next_turn' },
        text,
      });
      if (!submitted.ok) return { error: `submit: ${JSON.stringify(submitted)}` };
      return { workspaceId, sessionId, mode: mode.value.mode, profileId };
    },
    { selectedRoot: workspaceRoot, url: providerUrl, title: message.title, text: message.text },
  );
}
