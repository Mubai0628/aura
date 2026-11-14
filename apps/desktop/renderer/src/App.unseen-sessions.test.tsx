import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, workspaceSessionProjection } from './app-test-fixtures';

installAppTestEnvironment();

const OTHER_WORKSPACE = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const OTHER_SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';

function twoWorkspaces() {
  vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
    ok: true,
    value: {
      ...workspaceSessionProjection,
      payload: {
        sessions: [
          ...workspaceSessionProjection.payload.sessions,
          {
            session_id: OTHER_SESSION,
            workspace_id: OTHER_WORKSPACE,
            title: '另一个工作区的会话',
            state: 'idle',
          },
        ],
        workspaces: [
          ...workspaceSessionProjection.payload.workspaces,
          {
            workspace_id: OTHER_WORKSPACE,
            display_name: '第二个工作区',
            selected_root: 'C:/synthetic-second',
            canonical_root: 'C:/synthetic-second',
            authorization_epoch: '1',
            revision: '1',
            catalog_freshness: 'unavailable',
            state: 'online',
          },
        ],
      },
    },
  });
}

test('a conversation whose notification was not looked at is red in the sidebar, and its closed workspace is marked', async () => {
  twoWorkspaces();
  let send: ((sessionIds: string[]) => void) | undefined;
  const stop = vi.fn();
  Object.assign(window.aura.v1, {
    notifications: {
      setForegroundSession: vi.fn(),
      onNotificationOpen: () => () => undefined,
      onUnseenSessions: (listener: (sessionIds: string[]) => void) => {
        send = listener;
        return stop;
      },
    },
  });
  const user = userEvent.setup();
  const view = render(<App />);
  const expand = await screen.findByRole('button', { name: '展开 第二个工作区' });
  expect(screen.queryByRole('img', { name: '有新通知' })).toBeNull();
  act(() => send!([OTHER_SESSION]));
  expect(screen.getByRole('img', { name: '有新通知' })).toBeTruthy();
  await user.click(expand);
  const row = screen.getByRole('button', { name: /^另一个工作区的会话\s*有新通知$/u });
  const dot = () => row.querySelector('.session-state')!;
  expect(dot().classList.contains('has-notice')).toBe(true);
  // Open, the workspace shows the conversation's own dot instead.
  expect(screen.queryByRole('img', { name: '有新通知' })).toBeNull();
  // Looked at: Main says so, and the dot goes with the one on the taskbar button.
  act(() => send!([]));
  expect(dot().classList.contains('has-notice')).toBe(false);
  view.unmount();
  expect(stop).toHaveBeenCalled();
});
