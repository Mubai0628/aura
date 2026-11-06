import type { Page } from '@playwright/test';
import { readdir, readFile } from 'node:fs/promises';
import { join } from 'node:path';

/**
 * A workspace with one conversation, and a provider of our own with two models: one ready to use
 * and one still as it was added. Nothing here asks a model anything.
 */
export async function armOwnerRound(page: Page, workspaceRoot: string) {
  return page.evaluate(async (selectedRoot) => {
    const api = window.aura.v1;
    const fail = (step: string, reply: unknown) => ({
      error: `${step}: ${JSON.stringify(reply)}`,
    });
    const workspace = await api.createWorkspace({
      display_name: 'L18.11',
      selected_root: selectedRoot,
    });
    if (!workspace.ok) return fail('workspace', workspace);
    const session = await api.createSession({
      workspace_id: workspace.value.workspace.workspace_id,
      title: '',
    });
    if (!session.ok) return fail('session', session);
    const settings = await api.getModelSettings();
    if (!settings.ok) return fail('model settings', settings);
    const provider = await api.createModelProvider({
      expected_revision: settings.value.models_revision,
      input: { display_name: 'L18.11 服务', source: { type: 'custom', catalog_provider_id: null } },
    });
    if (!provider.ok) return fail('provider', provider);
    const endpoint = await api.createModelEndpoint({
      expected_revision: provider.value.revision,
      provider_id: provider.value.value.id,
      display_name: 'L18.11 连接',
      api_format: 'open_ai_chat_completions',
      base_url: 'http://127.0.0.1:9/v1',
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
    if (!endpoint.ok) return fail('endpoint', endpoint);
    let revision = endpoint.value.revision;
    const ids: string[] = [];
    for (const [display_name, provider_model_name] of [
      ['就绪模型', 'ready-1'],
      ['草稿模型', 'draft-1'],
    ] as const) {
      const profile = await api.createModelProfile({
        expected_revision: revision,
        input: {
          endpoint_id: endpoint.value.value.id,
          provider_model_name,
          display_name,
          notes: null,
          context_window_tokens: 128_000,
        },
      });
      if (!profile.ok) return fail(`profile ${display_name}`, profile);
      revision = profile.value.revision;
      ids.push(profile.value.value.id);
    }
    const ready = await api.updateModelProfile({
      expected_revision: revision,
      model_profile_id: ids[0]!,
      display_name: '就绪模型',
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
    if (!ready.ok) return fail('ready model', ready);
    return { sessionId: session.value.session_id };
  }, workspaceRoot);
}

/** Every event Main logged in this case's isolated data root. */
export async function mainEvents(root: string) {
  const namespaces = join(root, 'Local', 'Aura', 'namespaces');
  const events: { event: string; fields?: Record<string, unknown> }[] = [];
  for (const namespace of await readdir(namespaces).catch(() => [])) {
    const log = await readFile(join(namespaces, namespace, 'logs', 'main.jsonl'), 'utf8').catch(
      () => '',
    );
    for (const line of log.split('\n')) {
      if (line.trim() !== '') events.push(JSON.parse(line) as (typeof events)[number]);
    }
  }
  return events;
}

/** Failure codes Main logged, counted by code and command. */
export function failures(events: Awaited<ReturnType<typeof mainEvents>>) {
  const counted: Record<string, number> = {};
  for (const { fields } of events) {
    if (typeof fields?.code !== 'string') continue;
    const key = `${fields.code} ${String(fields.command ?? '')}`.trim();
    counted[key] = (counted[key] ?? 0) + 1;
  }
  return counted;
}
