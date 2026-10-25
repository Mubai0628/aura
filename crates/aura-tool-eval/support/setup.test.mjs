import assert from 'node:assert/strict';
import { test } from 'node:test';
import { setup } from './setup.mjs';

test('isolated setup mirrors owner request settings and only substitutes transport boundary', async () => {
  const captured = {};
  const answer = (value) => ({ ok: true, value });
  const revision = '1';
  const api = {
    createWorkspace: async () => answer({ workspace: { workspace_id: 'workspace' } }),
    createSession: async () => answer({ session_id: 'session' }),
    getSessionPermissionState: async () => answer({ revision }),
    setSessionPermissionMode: async () => answer({}),
    getModelSettings: async () => answer({ models_revision: revision }),
    createModelProvider: async () => answer({ revision, value: { id: 'provider' } }),
    createModelEndpoint: async (input) => {
      captured.endpoint = input;
      return answer({ revision, value: { id: 'endpoint' } });
    },
    createModelProfile: async (input) => {
      captured.profile = input;
      return answer({ revision, value: { id: 'profile' } });
    },
    updateModelProfile: async (input) => {
      captured.update = input;
      return answer({});
    },
    setSessionModelBinding: async () => answer({}),
    submitAgentMessage: async () => answer({}),
  };
  globalThis.window = { aura: { v1: api } };
  try {
    const request_settings = {
      api_format: 'open_ai_responses',
      request_defaults: {
        max_output_tokens: 8192,
        temperature: null,
        top_p: 0.98,
        chat_output_token_field: null,
        chat_instruction_role: null,
      },
      context_window_tokens: 256000,
      historical_snapshot_max_tokens: 1024,
      chat_output_token_field: null,
      chat_instruction_role: null,
      openai_organization_id: null,
      openai_project_id: null,
      max_parallel_model_requests: 2,
    };
    const timeouts = {
      connect_ms: 5000,
      first_event_ms: 420000,
      stream_idle_ms: 420000,
      total_ms: 450000,
    };
    await setup(
      { evaluate: (callback, config) => callback(config) },
      {
        workspace: 'isolated-copy',
        mode: 'workspace_read_write',
        provider_url: 'http://127.0.0.1:1234/v1',
        model: 'deepseek-flash',
        prompt: 'synthetic fixture',
        request_settings,
        timeouts,
      },
    );
    assert.equal(captured.endpoint.api_format, 'open_ai_responses');
    assert.equal(captured.endpoint.auth_scheme, 'none');
    assert.deepEqual(captured.endpoint.timeouts, timeouts);
    assert.equal(captured.endpoint.max_parallel_model_requests, 2);
    assert.equal(captured.profile.input.context_window_tokens, 256000);
    assert.deepEqual(captured.update.request_defaults, request_settings.request_defaults);
    assert.equal(captured.update.historical_snapshot_max_tokens, 1024);
  } finally {
    delete globalThis.window;
  }
});
