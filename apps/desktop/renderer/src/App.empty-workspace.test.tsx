import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, workspaceSessionProjection } from './app-test-fixtures';

installAppTestEnvironment();

describe('a workspace without conversations', () => {
  test('opens the new-conversation page instead of an empty card', async () => {
    vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
      ok: true,
      value: {
        ...workspaceSessionProjection,
        payload: { ...workspaceSessionProjection.payload, sessions: [] },
      },
    });
    render(<App />);
    expect(await screen.findByRole('heading', { level: 1, name: '从一个想法开始' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '还没有会话' })).toBeNull();
  });

  // Writing in it keeps the page.
  test('keeps the new-conversation page while the first message is written', async () => {
    const user = userEvent.setup();
    vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
      ok: true,
      value: {
        ...workspaceSessionProjection,
        payload: { ...workspaceSessionProjection.payload, sessions: [] },
      },
    });
    render(<App />);
    await screen.findByRole('heading', { level: 1, name: '从一个想法开始' });
    await user.type(screen.getByRole('textbox', { name: /向 Aura 提问|消息/u }), '写一个小游戏');
    expect(screen.getByRole('heading', { level: 1, name: '从一个想法开始' })).toBeTruthy();
    expect(screen.queryByRole('heading', { name: '还没有会话' })).toBeNull();
  });
});
