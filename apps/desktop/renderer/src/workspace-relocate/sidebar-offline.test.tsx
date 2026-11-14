import { render, screen } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import { App } from '../App';
import { installAppTestEnvironment, workspaceSessionProjection } from '../app-test-fixtures';

installAppTestEnvironment();

test('only a Workspace Core could not open at its folder shows 选择新位置 on its row', async () => {
  const [found] = workspaceSessionProjection.payload.workspaces;
  vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
    ok: true,
    value: {
      ...workspaceSessionProjection,
      payload: {
        ...workspaceSessionProjection.payload,
        workspaces: [
          { ...found!, state: 'offline' },
          {
            ...found!,
            workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAW',
            display_name: '在线的工作区',
            selected_root: 'C:/synthetic-online',
            canonical_root: 'C:/synthetic-online',
          },
        ],
      },
    },
  });
  render(<App />);
  expect(await screen.findByRole('button', { name: '为 合成工作区 选择新位置' })).toBeTruthy();
  expect(await screen.findByRole('button', { name: '在线的工作区 更多操作' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: '为 在线的工作区 选择新位置' })).toBeNull();
});
