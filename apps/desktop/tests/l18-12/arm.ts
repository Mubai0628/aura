import type { Page } from '@playwright/test';

/**
 * Another provider of our own beside the one `armOwnerRound` adds: one connection with a model
 * ready to use and one still as it was added. Nothing here asks a model anything.
 */
export async function addSecondProvider(page: Page) {
  return page.evaluate(async () => {
    const api = window.aura.v1;
    const fail = (step: string, reply: unknown) => ({
      error: `${step}: ${JSON.stringify(reply)}`,
    });
    const settings = await api.getModelSettings();
    if (!settings.ok) return fail('model settings', settings);
    const provider = await api.createModelProvider({
      expected_revision: settings.value.models_revision,
      input: { display_name: 'L18.12 乙', source: { type: 'custom', catalog_provider_id: null } },
    });
    if (!provider.ok) return fail('provider', provider);
    const endpoint = await api.createModelEndpoint({
      expected_revision: provider.value.revision,
      provider_id: provider.value.value.id,
      display_name: '乙连接',
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
      ['乙就绪', 'ready-2'],
      ['乙草稿', 'draft-2'],
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
      display_name: '乙就绪',
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
    return { ok: true };
  });
}

/**
 * The distinct places `selector`'s thumb is drawn at, frame by frame, while `act` works it: the
 * thumb's x offset in each frame, from the start of `act` until it has been still for a while.
 */
export async function thumbTravel(page: Page, selector: string, act: () => Promise<void>) {
  await page.evaluate((thumbSelector) => {
    const offsets: number[] = [];
    Reflect.set(window, 'auraThumbOffsets', offsets);
    const started = performance.now();
    const sample = () => {
      const thumb = document.querySelector(thumbSelector);
      if (thumb !== null) offsets.push(new DOMMatrix(getComputedStyle(thumb).transform).m41);
      if (performance.now() - started < 1_000) requestAnimationFrame(sample);
    };
    requestAnimationFrame(sample);
  }, selector);
  await act();
  await page.waitForTimeout(1_100);
  return page.evaluate(() => Reflect.get(window, 'auraThumbOffsets') as number[]);
}
