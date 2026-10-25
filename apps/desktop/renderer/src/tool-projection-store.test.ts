import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import {
  resetScopedToolProjectionStoreForTests,
  subscribeScopedToolProjection,
} from './tool-projection-store';

const RUNTIME_ID = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const SESSION_ID = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

const activityChange = (revision: string) =>
  ({
    payload: {
      active_batches: [],
      older_activity_cursor: null,
      recent_calls: [],
      session_id: SESSION_ID,
    },
    projection_kind: 'session_tool_activity',
    projection_scope: { session_id: SESSION_ID, type: 'session' },
    revision,
    runtime_instance_id: RUNTIME_ID,
  }) as const;

describe('scoped Tool Projection store', () => {
  let publish: ((change: ReturnType<typeof activityChange>) => void) | undefined;

  beforeEach(() => {
    Object.defineProperty(window, 'aura', {
      configurable: true,
      value: {
        v1: {
          onToolProjection: vi.fn((listener) => {
            publish = listener;
            return vi.fn();
          }),
          refreshToolProjection: vi
            .fn()
            // The committed snapshot the surface first reads, then the newer one a gap
            // detected later resolves to.
            .mockResolvedValueOnce({ ok: true, value: activityChange('1') })
            .mockResolvedValue({ ok: true, value: activityChange('5') }),
          subscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
          unsubscribeToolProjection: vi.fn().mockResolvedValue({ ok: true, value: undefined }),
        },
      },
    });
  });

  afterEach(() => {
    resetScopedToolProjectionStoreForTests();
    vi.restoreAllMocks();
  });

  test('reference-counts one exact Tool kind and scope', () => {
    const route = {
      projection_kind: 'session_tool_activity',
      projection_scope: { session_id: SESSION_ID, type: 'session' },
    } as const;
    const first = vi.fn();
    const second = vi.fn();

    const stopFirst = subscribeScopedToolProjection(route, first);
    const stopSecond = subscribeScopedToolProjection(route, second);

    expect(window.aura.v1.subscribeToolProjection).toHaveBeenCalledTimes(1);
    publish?.(activityChange('1'));
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);

    stopFirst();
    expect(window.aura.v1.unsubscribeToolProjection).not.toHaveBeenCalled();
    stopSecond();
    expect(window.aura.v1.unsubscribeToolProjection).toHaveBeenCalledTimes(1);
    expect(window.aura.v1.unsubscribeToolProjection).toHaveBeenCalledWith(route);
  });

  test('reads the committed snapshot once when a surface subscribes after the change', async () => {
    // A projection committed before the surface opened — a Registry head built at
    // startup, for instance — is never pushed, so subscribing alone would leave it
    // undefined forever. The first subscriber reads the committed snapshot once.
    const route = {
      projection_kind: 'session_tool_activity',
      projection_scope: { session_id: SESSION_ID, type: 'session' },
    } as const;
    const listener = vi.fn();
    const stop = subscribeScopedToolProjection(route, listener);

    await vi.waitFor(() => {
      expect(window.aura.v1.refreshToolProjection).toHaveBeenCalledTimes(1);
      expect(listener).toHaveBeenCalledTimes(1);
    });
    expect(window.aura.v1.refreshToolProjection).toHaveBeenCalledWith(route);

    // The value is cached per exact Kind + Scope: a second consumer shares it instead
    // of asking Core again.
    const second = vi.fn();
    const stopSecond = subscribeScopedToolProjection(route, second);
    expect(window.aura.v1.refreshToolProjection).toHaveBeenCalledTimes(1);
    stop();
    stopSecond();
  });

  test('refreshes only the affected scope after a revision gap', async () => {
    const route = {
      projection_kind: 'session_tool_activity',
      projection_scope: { session_id: SESSION_ID, type: 'session' },
    } as const;
    const listener = vi.fn();
    const stop = subscribeScopedToolProjection(route, listener);
    // The initial snapshot read publishes revision 1.
    await vi.waitFor(() => {
      expect(window.aura.v1.refreshToolProjection).toHaveBeenCalledTimes(1);
      expect(listener).toHaveBeenCalledTimes(1);
    });
    // Revision 3 is two ahead of what the surface holds, so the store re-reads that
    // exact Kind + Scope instead of showing a value it cannot vouch for.
    publish?.(activityChange('3'));

    await vi.waitFor(() => {
      expect(window.aura.v1.refreshToolProjection).toHaveBeenCalledTimes(2);
      expect(listener).toHaveBeenCalledTimes(2);
    });
    expect(window.aura.v1.refreshToolProjection).toHaveBeenLastCalledWith(route);
    stop();
  });
});
