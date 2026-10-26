import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, workspaceSessionProjection } from './app-test-fixtures';

installAppTestEnvironment();

const SECOND_WORKSPACE = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAW';

function twoWorkspaces() {
  vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
    ok: true,
    value: {
      ...workspaceSessionProjection,
      payload: {
        sessions: [
          ...workspaceSessionProjection.payload.sessions,
          {
            session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW',
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

const throughSettings = async (user: ReturnType<typeof userEvent.setup>) => {
  await user.click(screen.getByRole('button', { name: '设置' }));
  await user.click(await screen.findByRole('button', { name: '返回会话' }));
};

describe('the workspaces opened in the sidebar', () => {
  test('stay open after a visit to Settings', async () => {
    twoWorkspaces();
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: '展开 第二个工作区' }));
    expect(screen.getByRole('button', { name: '另一个工作区的会话' })).toBeTruthy();
    await throughSettings(user);
    expect(await screen.findByRole('button', { name: '另一个工作区的会话' })).toBeTruthy();
    expect(
      screen.getByRole('button', { name: '收起 第二个工作区' }).getAttribute('aria-expanded'),
    ).toBe('true');
  });

  test('a closed one stays closed, even the selected one', async () => {
    twoWorkspaces();
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole('button', { name: '收起 合成工作区' }));
    await throughSettings(user);
    await screen.findByRole('button', { name: '展开 合成工作区' });
    expect(screen.queryByRole('button', { name: '合成桌面会话' })).toBeNull();
  });

  test('stay open at the next start', async () => {
    twoWorkspaces();
    const user = userEvent.setup();
    const { unmount } = render(<App />);
    await user.click(await screen.findByRole('button', { name: '展开 第二个工作区' }));
    unmount();
    render(<App />);
    expect(await screen.findByRole('button', { name: '另一个工作区的会话' })).toBeTruthy();
  });
});
