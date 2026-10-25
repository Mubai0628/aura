import { render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, workspaceSessionProjection } from './app-test-fixtures';
import { OPEN_SESSION_KEY } from './open-session';

installAppTestEnvironment();

const FIRST_WORKSPACE = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const SECOND_WORKSPACE = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const SAME_WORKSPACE_SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAX';
const SECOND_WORKSPACE_SESSION = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW';

function twoWorkspaces() {
  vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
    ok: true,
    value: {
      ...workspaceSessionProjection,
      payload: {
        sessions: [
          ...workspaceSessionProjection.payload.sessions,
          {
            session_id: SAME_WORKSPACE_SESSION,
            workspace_id: FIRST_WORKSPACE,
            title: '同一工作区的第二个会话',
            state: 'idle',
          },
          {
            session_id: SECOND_WORKSPACE_SESSION,
            workspace_id: SECOND_WORKSPACE,
            title: '另一个工作区的会话',
            state: 'idle',
          },
        ],
        workspaces: [
          ...workspaceSessionProjection.payload.workspaces,
          {
            workspace_id: SECOND_WORKSPACE,
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

const remember = (workspaceId: string, sessionId: string) =>
  window.localStorage.setItem(OPEN_SESSION_KEY, JSON.stringify({ workspaceId, sessionId }));

describe('the conversation open when Aura closed', () => {
  test('opens again at the next start', async () => {
    twoWorkspaces();
    remember(SECOND_WORKSPACE, SECOND_WORKSPACE_SESSION);
    render(<App />);
    expect(
      await screen.findByRole('heading', { level: 2, name: '另一个工作区的会话' }),
    ).toBeTruthy();
  });

  test('is kept the moment it opens', async () => {
    twoWorkspaces();
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: '同一工作区的第二个会话' }));
    await waitFor(() =>
      expect(JSON.parse(window.localStorage.getItem(OPEN_SESSION_KEY) ?? 'null')).toEqual({
        workspaceId: FIRST_WORKSPACE,
        sessionId: SAME_WORKSPACE_SESSION,
      }),
    );
  });

  test('one that is gone leaves its workspace, or the first one, open', async () => {
    twoWorkspaces();
    remember(SECOND_WORKSPACE, 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAZ');
    const { unmount } = render(<App />);
    expect(
      await screen.findByRole('heading', { level: 2, name: '另一个工作区的会话' }),
    ).toBeTruthy();
    unmount();
    remember('wsp_01ARZ3NDEKTSV4RRFFQ69G5FAZ', 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAZ');
    render(<App />);
    expect(await screen.findByRole('heading', { level: 2, name: '合成桌面会话' })).toBeTruthy();
  });
});
