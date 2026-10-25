import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AppSidebar } from './AppSidebar';
import { ACTIVITY_EVERY_MS } from './session-activity';

// The sidebar marks the
// conversations whose turn really is under way, as Core reports them, and follows them as they
// start and end.

const read = vi.fn();
beforeEach(() => {
  vi.useFakeTimers();
  read.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentSessionsActivity: read } },
  });
});
afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const sidebar = () =>
  render(
    <AppSidebar
      activeScreen="workspace"
      coreStatus={{ label: 'Aura Core 已就绪', tone: 'success' }}
      demoMode={false}
      onNewSession={vi.fn()}
      onNotify={vi.fn()}
      onOpenSettings={vi.fn()}
      onSelectSession={vi.fn()}
      onSelectWorkspace={vi.fn()}
      selectedSessionId="ses_c"
      selectedWorkspaceId="wsp_1"
      workspaces={[
        {
          id: 'wsp_1',
          name: 'Mini-Game',
          root: 'C:\\Mini-Game',
          sessions: [
            { id: 'ses_a', state: 'active', title: '你好' },
            { id: 'ses_b', state: 'active', title: '分支：你好' },
            { id: 'ses_c', state: 'active', title: '新会话' },
          ],
        },
      ]}
    />,
  );

const dot = (title: string) =>
  screen.getByText(title).closest('button')!.querySelector('.session-state')!;

test('a running conversation turns its dot, a waiting one is amber, and both follow Core', async () => {
  read.mockResolvedValueOnce({
    ok: true,
    value: {
      sessions: [
        { session_id: 'ses_a', state: 'running' },
        { session_id: 'ses_b', state: 'waiting' },
      ],
    },
  });
  sidebar();
  await act(async () => {});
  expect(dot('你好').className).toContain('is-running');
  expect(dot('分支：你好').className).toContain('is-waiting');
  expect(dot('新会话').className).not.toMatch(/is-(running|waiting)/u);
  expect(screen.getByText('正在运行').className).toBe('sr-only');

  // The turn ends: the next look has nothing under way.
  read.mockResolvedValueOnce({ ok: true, value: { sessions: [] } });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ACTIVITY_EVERY_MS);
  });
  expect(dot('你好').className).not.toContain('is-running');
  expect(read).toHaveBeenCalledTimes(2);
});

test('a hidden window does not ask', async () => {
  const hidden = vi.spyOn(document, 'hidden', 'get').mockReturnValue(true);
  read.mockResolvedValue({ ok: true, value: { sessions: [] } });
  sidebar();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ACTIVITY_EVERY_MS * 3);
  });
  expect(read).not.toHaveBeenCalled();
  hidden.mockRestore();
});
