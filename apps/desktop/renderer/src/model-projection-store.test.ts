import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import {
  resetScopedModelProjectionStoreForTests,
  readSessionModelSelector,
  subscribeScopedModelProjection,
} from './model-projection-store';

describe('scoped model projection store', () => {
  let publish: ((change: never) => void) | undefined;

  beforeEach(() => {
    Object.defineProperty(window, 'aura', {
      configurable: true,
      value: {
        v1: {
          onModelProjection: vi.fn((listener) => {
            publish = listener;
            return vi.fn();
          }),
          subscribeModelProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
          unsubscribeModelProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        },
      },
    });
  });

  afterEach(() => {
    resetScopedModelProjectionStoreForTests();
    vi.restoreAllMocks();
  });

  test('a failed subscription rejects selection preparation instead of leaving a loading placeholder', async () => {
    vi.mocked(window.aura.v1.subscribeModelProjection).mockRejectedValueOnce(new Error('offline'));
    await expect(readSessionModelSelector('ses_01ARZ3NDEKTSV4RRFFQ69G5FAV')).rejects.toThrow(
      '模型配置读取失败',
    );
    expect(window.aura.v1.unsubscribeModelProjection).toHaveBeenCalledOnce();
  });

  test('reference-counts an exact kind and scope route', () => {
    const route = {
      projection_kind: 'session_model_selector',
      projection_scope: {
        session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        type: 'session',
      },
    } as const;
    const first = vi.fn();
    const second = vi.fn();

    const stopFirst = subscribeScopedModelProjection(route, first);
    const stopSecond = subscribeScopedModelProjection(route, second);

    expect(window.aura.v1.subscribeModelProjection).toHaveBeenCalledTimes(1);
    publish?.({
      payload: {
        binding_revision: '1',
        default_model_profile_id: null,
        endpoints: [],
        models: [],
        projection_revision: '1',
        providers: [],
        selected_model_execution_readiness: {
          is_executable: false,
          issue_code: 'model.session.unbound',
        },
        selected_model_profile_id: null,
        session_id: route.projection_scope.session_id,
      },
      projection_kind: route.projection_kind,
      projection_scope: route.projection_scope,
      revision: '1',
      runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    } as never);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);

    stopFirst();
    expect(window.aura.v1.unsubscribeModelProjection).not.toHaveBeenCalled();
    stopSecond();
    expect(window.aura.v1.unsubscribeModelProjection).toHaveBeenCalledTimes(1);
    expect(window.aura.v1.unsubscribeModelProjection).toHaveBeenCalledWith(route);
  });

  test('treats an unsubscribe for an already released consumer as a no-op', () => {
    const route = {
      projection_kind: 'model_settings',
      projection_scope: { type: 'global' },
    } as const;
    const stop = subscribeScopedModelProjection(route, vi.fn());
    stop();
    stop();
    expect(window.aura.v1.unsubscribeModelProjection).toHaveBeenCalledTimes(1);
  });
});
