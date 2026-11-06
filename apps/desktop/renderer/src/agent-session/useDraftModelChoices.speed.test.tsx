import { cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { MODEL_PROFILE_ID, modelSettingsProjection } from '../app-test-fixtures';
import { useDraftModelChoices } from './useDraftModelChoices';

// A new conversation's composer offers the speed modes of a model whose settings allow them, as
// a conversation's composer does; a model that does not allow them offers none.

vi.mock('../model-projection-store', () => ({ useScopedModelProjection: () => undefined }));
afterEach(cleanup);

const offered = async (allowed: boolean) => {
  const settings = structuredClone(modelSettingsProjection);
  settings.registry.models[MODEL_PROFILE_ID]!.request_defaults.allow_speed_modes = allowed;
  settings.speed_modes = [
    {
      model_profile_id: MODEL_PROFILE_ID,
      standard_price: null,
      modes: [{ name: 'fast', price: null }],
    },
  ];
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getModelSettings: vi.fn(async () => ({ ok: true, value: settings })) } },
  });
  const { result } = renderHook(() => useDraftModelChoices());
  await waitFor(() => expect(result.current.loading).toBe(false));
  return result.current.choices.find((choice) => choice.id === MODEL_PROFILE_ID)?.speedModes;
};

test('a new conversation offers the modes a model allows, and none for one that does not', async () => {
  expect(await offered(true)).toEqual(['fast']);
  cleanup();
  expect(await offered(false)).toEqual([]);
});
