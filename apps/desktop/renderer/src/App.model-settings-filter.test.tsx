import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import type { ModelSettingsProjection } from '@aura/contracts/generated';
import { App } from './App';
import {
  ANTHROPIC_PROVIDER_ID,
  installAppTestEnvironment,
  MODEL_ENDPOINT_ID,
  MODEL_PROFILE_ID,
  modelSettingsProjection,
  SECOND_MODEL_ENDPOINT_ID,
  SECOND_MODEL_PROFILE_ID,
} from './app-test-fixtures';

installAppTestEnvironment();

const NEEDS_LIMIT_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAX';
const READY_ID = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAY';

/** One connection with three models: two ready to use, one still needing its output limit. */
function threeModels(): ModelSettingsProjection {
  const projection = structuredClone(modelSettingsProjection) as ModelSettingsProjection;
  const original = projection.registry.models[MODEL_PROFILE_ID]!;
  projection.registry.models[NEEDS_LIMIT_ID] = {
    ...structuredClone(original),
    catalog: null,
    display_name: 'gpt-image-2',
    id: NEEDS_LIMIT_ID,
    provider_model_name: 'gpt-image-2',
  };
  projection.registry.models[READY_ID] = {
    ...structuredClone(original),
    display_name: 'Luna',
    id: READY_ID,
    provider_model_name: 'gpt-6-luna',
  };
  projection.registry.endpoints[MODEL_ENDPOINT_ID]!.model_order = [
    MODEL_PROFILE_ID,
    NEEDS_LIMIT_ID,
    READY_ID,
  ];
  return projection;
}

/** Besides OpenAI's three, Anthropic with one connection and two models, one of them ready. */
function twoProviders(): ModelSettingsProjection {
  const projection = threeModels();
  const openai = projection.registry.endpoints[MODEL_ENDPOINT_ID]!;
  projection.registry.endpoints[SECOND_MODEL_ENDPOINT_ID] = {
    ...structuredClone(openai),
    display_name: 'Anthropic 主连接',
    id: SECOND_MODEL_ENDPOINT_ID,
    model_order: [SECOND_MODEL_PROFILE_ID, `${SECOND_MODEL_PROFILE_ID.slice(0, -1)}Z`],
    provider_id: ANTHROPIC_PROVIDER_ID,
    slug: 'anthropic-main',
  };
  const ready = projection.registry.models[MODEL_PROFILE_ID]!;
  projection.registry.models[SECOND_MODEL_PROFILE_ID] = {
    ...structuredClone(ready),
    display_name: 'Claude Sonnet',
    endpoint_id: SECOND_MODEL_ENDPOINT_ID,
    id: SECOND_MODEL_PROFILE_ID,
    provider_model_name: 'claude-sonnet',
  };
  const unready = `${SECOND_MODEL_PROFILE_ID.slice(0, -1)}Z`;
  projection.registry.models[unready] = {
    ...structuredClone(ready),
    catalog: null,
    display_name: 'claude-draft',
    endpoint_id: SECOND_MODEL_ENDPOINT_ID,
    id: unready,
    provider_model_name: 'claude-draft',
  };
  projection.registry.provider_order.push(ANTHROPIC_PROVIDER_ID);
  projection.registry.providers[ANTHROPIC_PROVIDER_ID] = {
    ...structuredClone(projection.registry.providers[openai.provider_id]!),
    display_name: 'Anthropic',
    endpoint_order: [SECOND_MODEL_ENDPOINT_ID],
    id: ANTHROPIC_PROVIDER_ID,
    slug: 'anthropic',
    source: { catalog_provider_id: 'anthropic', type: 'models_dev' },
  };
  return projection;
}

async function openModels(projection: ModelSettingsProjection = threeModels()) {
  vi.mocked(window.aura.v1.getModelSettings).mockResolvedValue({ ok: true, value: projection });
  const user = userEvent.setup();
  render(<App />);
  await user.click(screen.getByRole('button', { name: '设置' }));
  const section = await screen.findByRole('region', { name: 'OpenAI 模型' });
  await within(section).findAllByText('gpt-image-2');
  const shown = () =>
    within(section)
      .queryAllByRole('article')
      .map((card) => card.querySelector('.model-card-title strong')?.textContent);
  return { section, shown, user };
}

describe('the models of a provider, filtered', () => {
  test('只看已配置 leaves the models ready to use and counts what is shown', async () => {
    const { section, shown, user } = await openModels();
    expect(shown()).toEqual(['gpt-5.6-sol', 'gpt-image-2', 'Luna']);
    await user.click(within(section).getByRole('switch', { name: '只看已配置' }));
    expect(shown()).toEqual(['gpt-5.6-sol', 'Luna']);
    expect(within(section).getByText('2/3 个')).toBeTruthy();
    await user.click(within(section).getByRole('switch', { name: '只看已配置' }));
    expect(shown()).toHaveLength(3);
    expect(within(section).getByText('3 个')).toBeTruthy();
  });

  test('搜索 matches a model by its name or its model id', async () => {
    const { section, shown, user } = await openModels();
    const search = within(section).getByRole('searchbox', { name: '搜索模型' });
    await user.type(search, 'LUNA');
    expect(shown()).toEqual(['Luna']);
    await user.clear(search);
    await user.type(search, 'image');
    expect(shown()).toEqual(['gpt-image-2']);
    await user.click(within(section).getByRole('switch', { name: '只看已配置' }));
    expect(shown()).toEqual([]);
    expect(within(section).getByText('没有符合的模型')).toBeTruthy();
  });

  test("只看已配置 is each provider's own, shared by its connections", async () => {
    const { section, shown, user } = await openModels(twoProviders());
    const onlyReady = () => screen.getByRole('switch', { name: '只看已配置' });
    const cards = (region: HTMLElement) =>
      within(region)
        .queryAllByRole('article')
        .map((card) => card.querySelector('.model-card-title strong')?.textContent);
    await user.click(within(section).getByRole('switch', { name: '只看已配置' }));
    expect(shown()).toEqual(['gpt-5.6-sol', 'Luna']);
    await user.click(screen.getByRole('button', { name: /^Anthropic/u }));
    const anthropic = await screen.findByRole('region', { name: 'Anthropic 模型' });
    expect(onlyReady().getAttribute('aria-checked')).toBe('false');
    expect(cards(anthropic)).toEqual(['Claude Sonnet', 'claude-draft']);
    await user.click(screen.getByRole('button', { name: /^OpenAI/u }));
    const openai = await screen.findByRole('region', { name: 'OpenAI 模型' });
    expect(onlyReady().getAttribute('aria-checked')).toBe('true');
    expect(cards(openai)).toEqual(['gpt-5.6-sol', 'Luna']);
  });
});
