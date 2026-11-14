import type { ModelSettingsProjection } from '@aura/contracts/generated';
import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { MODEL_ENDPOINT_ID, MODEL_PROFILE_ID, modelSettingsProjection } from '../app-test-fixtures';
import { useDraftModelChoices } from './useDraftModelChoices';

// A new conversation's model menu listed models that were not ready as plainly as ready ones,
// while a conversation's own menu dims them: a model whose connection has no usable credential,
// or that has no output limit, can be told apart before the first message.

vi.mock('../model-projection-store', () => ({ useScopedModelProjection: () => undefined }));
afterEach(cleanup);

const reasonFor = async (change: (settings: ModelSettingsProjection) => void) => {
  const settings = structuredClone(modelSettingsProjection);
  change(settings);
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getModelSettings: vi.fn(async () => ({ ok: true, value: settings })) } },
  });
  const { result } = renderHook(() => useDraftModelChoices());
  await waitFor(() => expect(result.current.loading).toBe(false));
  const choice = result.current.choices.find((item) => item.id === MODEL_PROFILE_ID);
  cleanup();
  return choice?.disabledReason;
};

test('a new conversation dims the models it cannot use yet, as a conversation does', async () => {
  expect(await reasonFor(() => undefined)).toBeUndefined();
  expect(
    await reasonFor((settings) => {
      settings.active_credentials = [];
    }),
  ).toBe('凭据未配置');
  expect(
    await reasonFor((settings) => {
      settings.active_credentials[0]!.state = 'cleared';
    }),
  ).toBe('凭据已清除');
  expect(
    await reasonFor((settings) => {
      const model = settings.registry.models[MODEL_PROFILE_ID]!;
      model.request_defaults.max_output_tokens = null;
      if (model.catalog) model.catalog.snapshot.output_limit_tokens = null;
    }),
  ).toBe('需配置输出上限');
  expect(
    await reasonFor((settings) => {
      settings.registry.models[MODEL_PROFILE_ID]!.enabled = false;
    }),
  ).toBe('模型已停用');
  // A connection that needs no credential is ready without one.
  expect(
    await reasonFor((settings) => {
      settings.active_credentials = [];
      settings.registry.endpoints[MODEL_ENDPOINT_ID]!.auth_scheme = 'none';
    }),
  ).toBeUndefined();
});
