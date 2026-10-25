import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import {
  UNAVAILABLE_UPDATE_STATE,
  type AppUpdateCommand,
  type AppUpdateState,
} from '../../../shared/app-updates';
import { useAppUpdates } from './useAppUpdates';

const state = (change: Partial<AppUpdateState> = {}): AppUpdateState => ({
  ...UNAVAILABLE_UPDATE_STATE,
  available: true,
  current: '1.0.0',
  auto_check: true,
  ...change,
});
function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (error: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function bridge() {
  const order: string[] = [],
    listeners = new Set<(state: AppUpdateState) => void>();
  const unsubscribe = vi.fn();
  const appUpdate = vi.fn<(command: AppUpdateCommand) => Promise<AppUpdateState>>(async () =>
    state(),
  );
  const onAppUpdateState = vi.fn((listener: (state: AppUpdateState) => void) => {
    order.push('subscribe');
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
      unsubscribe();
    };
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { appUpdate, onAppUpdateState } },
  });
  return {
    appUpdate,
    onAppUpdateState,
    unsubscribe,
    listeners,
    order,
    push: (value: AppUpdateState) => act(() => listeners.forEach((listener) => listener(value))),
  };
}
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

test('notification and settings consumers share one subscription, installed before the initial read', async () => {
  const api = bridge(),
    initial = deferred<AppUpdateState>();
  api.appUpdate.mockImplementation(() => {
    api.order.push('state');
    return initial.promise;
  });
  const first = renderHook(useAppUpdates),
    second = renderHook(useAppUpdates);
  expect(api.order).toEqual(['subscribe', 'state']);
  expect(api.onAppUpdateState).toHaveBeenCalledTimes(1);
  expect(api.appUpdate).toHaveBeenCalledTimes(1);
  await act(async () => initial.resolve(state()));
  expect(first.result.current.state).toEqual(state());
  const pushed = state({ phase: 'checking' });
  api.push(pushed);
  expect(first.result.current.state).toEqual(pushed);
  expect(second.result.current.state).toEqual(pushed);
  first.unmount();
  expect(api.unsubscribe).not.toHaveBeenCalled();
  second.unmount();
  expect(api.unsubscribe).toHaveBeenCalledTimes(1);
});

test('a push during the initial read takes precedence over its stale response', async () => {
  const api = bridge(),
    initial = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValue(initial.promise);
  const hook = renderHook(useAppUpdates),
    pushed = state({
      phase: 'available',
      latest: { version: '2.0.0', size: 20, published_at: null, notes: '' },
    });
  api.push(pushed);
  await act(async () => initial.resolve(state()));
  expect(hook.result.current.state).toEqual(pushed);
});

test('an immediate subscription push also wins over the subsequent initial response', async () => {
  const api = bridge(),
    pushed = state({ phase: 'paused', progress: { done: 10, total: 50 } });
  api.onAppUpdateState.mockImplementation((listener) => {
    listener(pushed);
    return api.unsubscribe;
  });
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  expect(hook.result.current.state).toEqual(pushed);
});

test('the preload unavailable fallback is a failed acknowledgement and preserves the last confirmed settings', async () => {
  const api = bridge();
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  api.appUpdate.mockResolvedValueOnce(UNAVAILABLE_UPDATE_STATE);
  let accepted = true;
  await act(async () => {
    accepted = await hook.result.current.send({ command: 'set_auto_check', enabled: false });
  });
  expect(accepted).toBe(false);
  expect(hook.result.current.state?.auto_check).toBe(true);
  expect(hook.result.current.error).toContain('重试');
});

test('a command RPC rejection after Main pushed a setting keeps that setting and reports its failed acknowledgement', async () => {
  const api = bridge();
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  const request = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValueOnce(request.promise);
  let sent!: Promise<boolean>;
  act(() => {
    sent = hook.result.current.send({ command: 'set_auto_check', enabled: false });
  });
  api.push(state({ auto_check: false }));
  await act(async () => request.reject(new Error('permission denied saving settings.json')));
  expect(await sent).toBe(false);
  expect(hook.result.current.state?.auto_check).toBe(false);
  expect(hook.result.current.error).toContain('重试');
});

test('an obsolete initial read rejection does not invalidate a valid pushed state', async () => {
  const api = bridge(),
    initial = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValueOnce(initial.promise);
  const hook = renderHook(useAppUpdates);
  api.push(state({ phase: 'paused' }));
  await act(async () => initial.reject(new Error('old read failed')));
  expect(hook.result.current.state?.phase).toBe('paused');
  expect(hook.result.current.error).toBeUndefined();
});

test('command responses cannot overwrite newer pushed state and all consumers share the lock', async () => {
  const api = bridge();
  const first = renderHook(useAppUpdates),
    second = renderHook(useAppUpdates);
  await act(async () => {});
  const result = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValue(result.promise);
  let sent!: Promise<boolean>;
  act(() => {
    sent = first.result.current.send({ command: 'check' });
  });
  expect(first.result.current.pending).toBe('check');
  expect(second.result.current.pending).toBe('check');
  await expect(
    second.result.current.send({ command: 'set_auto_check', enabled: false }),
  ).resolves.toBe(false);
  const pushed = state({ phase: 'up_to_date', checked_at: '2026-10-25T01:00:00Z' });
  api.push(pushed);
  await act(async () => result.resolve(state({ phase: 'available' })));
  expect(await sent).toBe(true);
  expect(first.result.current.state).toEqual(pushed);
  expect(second.result.current.pending).toBeUndefined();
  expect(api.appUpdate).toHaveBeenCalledTimes(2);
});

test('auto check changes only when Main confirms it and rejected RPCs expose a safe retryable Chinese error', async () => {
  const api = bridge();
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  const update = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValueOnce(update.promise);
  let sent!: Promise<boolean>;
  act(() => {
    sent = hook.result.current.send({ command: 'set_auto_check', enabled: false });
  });
  expect(hook.result.current.state?.auto_check).toBe(true);
  await act(async () => update.reject(new Error('secret token /private/config.json')));
  expect(await sent).toBe(false);
  expect(hook.result.current.state?.auto_check).toBe(true);
  expect(hook.result.current.error).toMatch(/设置|保存/u);
  expect(hook.result.current.error).toContain('重试');
  expect(hook.result.current.error).not.toMatch(/secret|private|token/u);
  expect(hook.result.current.pending).toBeUndefined();
  api.appUpdate.mockResolvedValueOnce(state({ auto_check: false }));
  let retried = false;
  await act(async () => {
    retried = await hook.result.current.send({ command: 'set_auto_check', enabled: false });
  });
  expect(retried).toBe(true);
  expect(hook.result.current.state?.auto_check).toBe(false);
  expect(hook.result.current.error).toBeUndefined();
});

test('pause can interrupt the complete download RPC without inventing a paused phase or accepting its late response', async () => {
  const api = bridge();
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  const downloading = deferred<AppUpdateState>(),
    pause = deferred<AppUpdateState>();
  api.appUpdate.mockImplementation((command) =>
    command.command === 'download' ? downloading.promise : pause.promise,
  );
  let downloaded!: Promise<boolean>, paused!: Promise<boolean>;
  act(() => {
    downloaded = hook.result.current.send({ command: 'download' });
  });
  api.push(state({ phase: 'downloading', progress: { done: 5, total: 50 } }));
  act(() => {
    paused = hook.result.current.send({ command: 'pause' });
  });
  expect(api.appUpdate.mock.calls.map(([command]) => command.command)).toEqual([
    'state',
    'download',
    'pause',
  ]);
  expect(hook.result.current.pending).toBe('pause');
  await expect(hook.result.current.send({ command: 'check' })).resolves.toBe(false);
  await act(async () =>
    pause.resolve(state({ phase: 'downloading', progress: { done: 5, total: 50 } })),
  );
  expect(await paused).toBe(true);
  expect(hook.result.current.state?.phase).toBe('downloading');
  expect(hook.result.current.pending).toBe('download');
  const stopped = state({ phase: 'paused', progress: { done: 5, total: 50 } });
  api.push(stopped);
  await act(async () => downloading.resolve(state({ phase: 'ready', waiting: '2.0.0' })));
  expect(await downloaded).toBe(true);
  expect(hook.result.current.state).toEqual(stopped);
  expect(hook.result.current.pending).toBeUndefined();
});

test('pause is not allowed to interrupt applying and no second pause overlaps the first', async () => {
  const api = bridge();
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  const download = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValue(download.promise);
  act(() => {
    void hook.result.current.send({ command: 'download' });
  });
  api.push(state({ phase: 'applying' }));
  await expect(hook.result.current.send({ command: 'pause' })).resolves.toBe(false);
  api.push(state({ phase: 'downloading' }));
  let pause!: Promise<boolean>;
  act(() => {
    pause = hook.result.current.send({ command: 'pause' });
  });
  await expect(hook.result.current.send({ command: 'pause' })).resolves.toBe(false);
  await act(async () => download.resolve(state({ phase: 'paused' })));
  await pause;
  expect(api.appUpdate).toHaveBeenCalledTimes(3);
});

test('initial read failures can be explicitly retried without leaking the rejected exception', async () => {
  const api = bridge();
  api.appUpdate.mockRejectedValueOnce(new Error('raw node exception C:/owner/profile'));
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  expect(hook.result.current.error).toContain('重试');
  expect(hook.result.current.error).not.toMatch(/node|owner/u);
  let recovered = false;
  await act(async () => {
    recovered = await hook.result.current.send({ command: 'state' });
  });
  expect(recovered).toBe(true);
  expect(hook.result.current.state).toEqual(state());
  expect(hook.result.current.error).toBeUndefined();
});

test('unmount and remount reject old reads and detached push callbacks', async () => {
  const api = bridge(),
    old = deferred<AppUpdateState>(),
    fresh = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValueOnce(old.promise).mockReturnValueOnce(fresh.promise);
  const first = renderHook(useAppUpdates),
    detached = [...api.listeners][0]!;
  first.unmount();
  const second = renderHook(useAppUpdates);
  await act(async () => fresh.resolve(state({ phase: 'up_to_date' })));
  act(() => detached(state({ phase: 'available' })));
  await act(async () => old.resolve(state({ phase: 'error', error: 'obsolete' })));
  expect(second.result.current.state?.phase).toBe('up_to_date');
  expect(second.result.current.error).toBeUndefined();
  expect(api.listeners.size).toBe(1);
});

test('a detached command keeps the global lock until it settles but cannot change remounted state', async () => {
  const api = bridge();
  const first = renderHook(useAppUpdates);
  await act(async () => {});
  const old = deferred<AppUpdateState>();
  api.appUpdate.mockReturnValueOnce(old.promise);
  let sent!: Promise<boolean>;
  act(() => {
    sent = first.result.current.send({ command: 'check' });
  });
  first.unmount();
  const second = renderHook(useAppUpdates);
  await act(async () => {});
  expect(second.result.current.pending).toBe('check');
  await expect(second.result.current.send({ command: 'check' })).resolves.toBe(false);
  await act(async () => old.resolve(state({ phase: 'available' })));
  expect(await sent).toBe(false);
  expect(second.result.current.state?.phase).toBe('idle');
  expect(second.result.current.pending).toBeUndefined();
});

test('a failed rollback reports false and keeps Main error state rather than implying restart or cancellation', async () => {
  const api = bridge();
  const hook = renderHook(useAppUpdates);
  await act(async () => {});
  const failed = state({ phase: 'error', error: '安装程序没有完成' });
  api.appUpdate.mockResolvedValueOnce(failed);
  let succeeded = true;
  await act(async () => {
    succeeded = await hook.result.current.send({ command: 'roll_back', restore_data: true });
  });
  expect(succeeded).toBe(false);
  expect(hook.result.current.state).toEqual(failed);
  expect(api.appUpdate.mock.calls.map(([command]) => command.command)).toEqual([
    'state',
    'roll_back',
  ]);
});

test.each<AppUpdateCommand>([
  { command: 'restart' },
  { command: 'state' },
  { command: 'dismiss_notice' },
  { command: 'set_auto_check', enabled: false },
  { command: 'pause' },
])(
  'a normal %j acknowledgement is not rejected because it retains an earlier check error',
  async (command) => {
    const previousError = state({ phase: 'error', error: '上次检查失败', waiting: '2.0.0' });
    const api = bridge();
    api.appUpdate.mockResolvedValue(previousError);
    const hook = renderHook(useAppUpdates);
    await act(async () => {});
    const confirmed = {
      ...previousError,
      auto_check: command.command === 'set_auto_check' ? false : true,
    };
    api.appUpdate.mockResolvedValueOnce(confirmed);
    let accepted = false;
    await act(async () => {
      accepted = await hook.result.current.send(command);
    });
    expect(accepted).toBe(true);
    expect(hook.result.current.state).toEqual(confirmed);
    expect(hook.result.current.pending).toBeUndefined();
  },
);

test.each<AppUpdateCommand>([{ command: 'check' }, { command: 'download' }])(
  'a failed %j phase still reports false',
  async (command) => {
    const api = bridge();
    const hook = renderHook(useAppUpdates);
    await act(async () => {});
    const failed = state({ phase: 'error', error: '操作失败' });
    api.appUpdate.mockResolvedValueOnce(failed);
    let accepted = true;
    await act(async () => {
      accepted = await hook.result.current.send(command);
    });
    expect(accepted).toBe(false);
    expect(hook.result.current.state).toEqual(failed);
  },
);
