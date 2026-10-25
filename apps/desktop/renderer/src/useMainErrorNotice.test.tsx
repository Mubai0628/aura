import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import type { MainErrorNoticeV1 } from '../../shared/main-errors-api';
import { useMainErrorNotice } from './useMainErrorNotice';
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});
test('a single persistent Main error notice survives idle time, replaces older errors, and cannot be restored by a stale snapshot', async () => {
  vi.useFakeTimers();
  let deliver: ((notice: MainErrorNoticeV1) => void) | undefined;
  let snapshot: ((result: unknown) => void) | undefined;
  const unsubscribe = vi.fn();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        mainErrors: {
          onChange: (receive: typeof deliver) => {
            deliver = receive;
            return unsubscribe;
          },
          snapshot: () =>
            new Promise((resolve) => {
              snapshot = resolve;
            }),
        },
      },
    },
  });
  const hook = renderHook(() => useMainErrorNotice());
  const notice: MainErrorNoticeV1 = {
    version: 1,
    id: 'main_error_2',
    kind: 'uncaught_exception',
    logged: true,
    log_path: 'temporary/main.jsonl',
  };
  act(() => deliver!(notice));
  act(() => vi.advanceTimersByTime(10_000));
  expect(hook.result.current.item?.title).toBe('Aura 主进程发生错误');
  act(() => hook.result.current.dismiss(notice.id));
  await act(async () => snapshot!({ ok: true, value: { ...notice, id: 'main_error_1' } }));
  expect(hook.result.current.item).toBeUndefined();
  act(() => deliver!({ ...notice, id: 'main_error_3', logged: false }));
  expect(hook.result.current.item?.message).toContain('日志写入失败');
  hook.unmount();
  expect(unsubscribe).toHaveBeenCalledOnce();
});
