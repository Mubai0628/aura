import { act, cleanup, renderHook, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { modelSettingsProjection } from '../app-test-fixtures';
import { useDraftModelChoices } from './useDraftModelChoices';

// A new conversation's model list that could not be read says why and what that means, and
// opening the list again reads it again.

vi.mock('../model-projection-store', () => ({ useScopedModelProjection: () => undefined }));
afterEach(cleanup);

test('a model list not read says why, and a retry reads it again', async () => {
  const read = vi
    .fn()
    .mockResolvedValueOnce({
      ok: false,
      error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
    })
    .mockResolvedValueOnce({ ok: true, value: modelSettingsProjection });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { getModelSettings: read } },
  });
  const { result } = renderHook(() => useDraftModelChoices());
  await waitFor(() =>
    expect(result.current.error).toBe(
      '模型列表没有读到（Aura Core 没有及时回复），暂时不能选模型；关闭再打开这个列表会重新读取。',
    ),
  );
  act(() => result.current.retry());
  await waitFor(() => expect(result.current.choices.length).toBeGreaterThan(0));
  expect(result.current.error).toBeUndefined();
  expect(read).toHaveBeenCalledTimes(2);
});
