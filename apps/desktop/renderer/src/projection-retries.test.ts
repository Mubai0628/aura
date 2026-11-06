import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  resetScopedModelProjectionStoreForTests,
  subscribeScopedModelProjection,
  useModelProjectionError,
  useScopedModelProjection,
} from './model-projection-store';
import {
  resetScopedToolProjectionStoreForTests,
  useScopedToolProjection,
} from './tool-projection-store';

// A subscription Main refused because Core had not started, was restarting or did not answer was
// never asked again: the composer's model button stayed on "选择模型" until the conversation was
// opened again. It is asked again at once when Core says it is ready, and otherwise after a pause
// that doubles from 1 s up to 30 s.

const SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const RUNTIME = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const RESTARTED = 'run_01M5PF3QWJN5S3P0JFPVBSFY3R';
const NOT_READY = {
  ok: false,
  error: {
    code: 'core_not_ready',
    message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
  },
} as never;
const UNREACHABLE = {
  ok: false,
  error: {
    code: 'core_request_failed',
    message: 'Aura Core 暂时连不上（可能正在重启），这次操作没有发出；请稍后重试。',
  },
} as never;
const DONE = { ok: true, value: undefined } as never;

const selectorRoute = {
  projection_kind: 'session_model_selector',
  projection_scope: { session_id: SESSION, type: 'session' },
} as const;
const selector = (revision: string) => ({
  payload: {
    binding_revision: '1',
    default_model_profile_id: null,
    endpoints: [],
    models: [],
    projection_revision: revision,
    providers: [],
    selected_model_execution_readiness: {
      is_executable: false,
      issue_code: 'model.session.unbound',
    },
    selected_model_profile_id: null,
    session_id: SESSION,
  },
  projection_kind: selectorRoute.projection_kind,
  projection_scope: selectorRoute.projection_scope,
  revision,
  runtime_instance_id: RUNTIME,
});
const toolRoute = {
  projection_kind: 'tool_settings',
  projection_scope: { type: 'global' },
} as const;
const health = (state: string, runtime = RUNTIME) => ({
  payload: { state },
  projection_kind: 'core_health',
  projection_scope: { type: 'global' },
  revision: '1',
  runtime_instance_id: runtime,
});

let publishModel: ((change: unknown) => void) | undefined;
let publishTool: ((change: unknown) => void) | undefined;
let publishHealth: ((change: unknown) => void) | undefined;
/** Lets the answers already given reach the stores. */
const settle = () => act(async () => await new Promise((resolve) => setTimeout(resolve, 0)));

beforeEach(() => {
  publishModel = publishTool = publishHealth = undefined;
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        onModelProjection: vi.fn((listener) => {
          publishModel = listener;
          return vi.fn();
        }),
        onToolProjection: vi.fn((listener) => {
          publishTool = listener;
          return vi.fn();
        }),
        onCoreHealth: vi.fn((listener) => {
          publishHealth = listener;
          return () => {
            if (publishHealth === listener) publishHealth = undefined;
          };
        }),
        subscribeModelProjection: vi.fn(),
        unsubscribeModelProjection: vi.fn().mockResolvedValue(DONE),
        subscribeToolProjection: vi.fn(),
        unsubscribeToolProjection: vi.fn().mockResolvedValue(DONE),
        refreshToolProjection: vi.fn().mockResolvedValue(UNREACHABLE),
      },
    },
  });
  vi.spyOn(console, 'error').mockImplementation(() => undefined);
});
afterEach(() => {
  resetScopedModelProjectionStoreForTests();
  resetScopedToolProjectionStoreForTests();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

test('a conversation’s model refused before Core is ready is asked again once Core is ready', async () => {
  const subscribe = vi.mocked(window.aura.v1.subscribeModelProjection);
  subscribe.mockResolvedValueOnce(NOT_READY).mockResolvedValue(DONE);
  const shown = renderHook(() => ({
    error: useModelProjectionError(selectorRoute),
    value: useScopedModelProjection(selectorRoute),
  }));
  await settle();
  expect(shown.result.current.error).toContain('尚未就绪');
  expect(subscribe).toHaveBeenCalledTimes(1);

  // Still starting: nothing is asked yet.
  act(() => publishHealth?.(health('starting')));
  expect(subscribe).toHaveBeenCalledTimes(1);
  act(() => publishHealth?.(health('ready')));
  expect(subscribe).toHaveBeenCalledTimes(2);
  expect(subscribe).toHaveBeenLastCalledWith(selectorRoute);

  // Core answers the subscription with the conversation's current state.
  await settle();
  act(() => publishModel?.(selector('1')));
  expect(shown.result.current.value?.revision).toBe('1');
  expect(shown.result.current.error).toBeUndefined();
  // Nothing more is asked once the route holds.
  act(() => publishHealth?.(health('ready', RESTARTED)));
  expect(subscribe).toHaveBeenCalledTimes(2);
});

test('without word from Core, a refused route is asked again after a pause that doubles up to 30 s', async () => {
  vi.useFakeTimers();
  const subscribe = vi.mocked(window.aura.v1.subscribeModelProjection);
  subscribe.mockResolvedValue(NOT_READY);
  const stop = subscribeScopedModelProjection(selectorRoute, vi.fn());
  await vi.advanceTimersByTimeAsync(0);
  expect(subscribe).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(999);
  expect(subscribe).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(subscribe).toHaveBeenCalledTimes(2);
  await vi.advanceTimersByTimeAsync(2_000);
  expect(subscribe).toHaveBeenCalledTimes(3);
  await vi.advanceTimersByTimeAsync(4_000 + 8_000 + 16_000);
  expect(subscribe).toHaveBeenCalledTimes(6);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(subscribe).toHaveBeenCalledTimes(7);

  subscribe.mockResolvedValue(DONE);
  await vi.advanceTimersByTimeAsync(30_000);
  expect(subscribe).toHaveBeenCalledTimes(8);
  await vi.advanceTimersByTimeAsync(120_000);
  expect(subscribe).toHaveBeenCalledTimes(8);
  stop();
});

test('a refused route nobody shows any more is not asked again', async () => {
  vi.useFakeTimers();
  const subscribe = vi.mocked(window.aura.v1.subscribeModelProjection);
  subscribe.mockResolvedValue(NOT_READY);
  const stop = subscribeScopedModelProjection(selectorRoute, vi.fn());
  await vi.advanceTimersByTimeAsync(0);
  stop();
  publishHealth?.(health('ready'));
  await vi.advanceTimersByTimeAsync(120_000);
  expect(subscribe).toHaveBeenCalledTimes(1);
});

test('a Tool route refused while Core restarts is asked again once the new Core is ready', async () => {
  const subscribe = vi.mocked(window.aura.v1.subscribeToolProjection);
  subscribe.mockResolvedValueOnce(UNREACHABLE).mockResolvedValue(DONE);
  const shown = renderHook(() => useScopedToolProjection(toolRoute));
  await settle();
  expect(subscribe).toHaveBeenCalledTimes(1);
  expect(shown.result.current).toBeUndefined();

  act(() => publishHealth?.(health('restarting')));
  expect(subscribe).toHaveBeenCalledTimes(1);
  act(() => publishHealth?.(health('ready', RESTARTED)));
  expect(subscribe).toHaveBeenCalledTimes(2);
  expect(subscribe).toHaveBeenLastCalledWith(toolRoute);

  await settle();
  act(() =>
    publishTool?.({
      payload: { tools: [] },
      projection_kind: toolRoute.projection_kind,
      projection_scope: toolRoute.projection_scope,
      revision: '1',
      runtime_instance_id: RESTARTED,
    }),
  );
  expect(shown.result.current?.revision).toBe('1');
});
