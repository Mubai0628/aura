import { act, cleanup, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import {
  OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY,
  REQUEST_TIMEOUT_CODE,
} from '../../../shared/ipc-request-failure';
import { useComposerSubmission } from './useComposerSubmission';

// /init and /compact start a task of their own; a start that failed says which, why, that the
// draft is kept, and what to do, as a failed send does.

const init = vi.fn();
const compact = vi.fn();
beforeEach(() => {
  init.mockReset();
  compact.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { startWorkspaceInit: init, startAgentCompaction: compact } },
  });
});
afterEach(cleanup);

const hook = () =>
  renderHook(() => useComposerSubmission('session', undefined, 'model', false, vi.fn())).result;

test('an /init Core refused names the task, the cause and the next step', async () => {
  init.mockResolvedValueOnce({
    ok: false,
    error: { code: 'ipc.core_not_ready', message: 'Core is not ready' },
  });
  const result = hook();
  await act(() => result.current.sendInit());
  expect(result.current.error).toBe(
    'Aura Core 还没有就绪（正在启动或恢复），这次操作没有发出，什么都没改；就绪后再试。',
  );
  init.mockResolvedValueOnce({
    ok: false,
    error: { code: 'agent.workspace_init_refused', message: 'workspace init refused' },
  });
  await act(() => result.current.sendInit());
  expect(result.current.error).toBe('初始化没有开始，草稿已保留；请检查模型和连接后再试。');
});

test('a /compact whose answer did not come says it may have started and what to check', async () => {
  compact.mockResolvedValueOnce({
    ok: false,
    error: {
      code: REQUEST_TIMEOUT_CODE,
      message: 'Core request timed out',
      details: { outcome_recovery: OUTCOME_RECOVERY_QUERY_WITHOUT_REPLAY },
    },
  });
  const result = hook();
  await act(() => result.current.sendCompaction());
  expect(result.current.error).toBe(
    '没有等到 Aura Core 的答复，压缩可能已经开始；草稿已保留。请先看消息历史，确认没有开始再试。',
  );
  expect(result.current.needsReview).toBe(true);
});
