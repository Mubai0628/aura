import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { useWorkspaceSessions } from './workspace-session-store';

afterEach(cleanup);

const change = (revision: string, titles: string[]) => ({
  runtime_instance_id: 'run_A',
  projection_kind: 'workspace_session',
  projection_scope: { type: 'global' },
  revision,
  payload: {
    workspaces: [],
    sessions: titles.map((title, index) => ({
      session_id: `ses_${index}`,
      workspace_id: 'wsp_A',
      title,
      state: 'idle',
    })),
  },
});

function Titles({ label }: { label: string }) {
  const titles = useWorkspaceSessions().projection?.payload.sessions.map((s) => s.title) ?? [];
  return <p aria-label={label}>{titles.join(',')}</p>;
}

/**
 * The store is one per window. Leaving 设置 → 归档 (a second reader) unsubscribed it from
 * every later change, so the sidebar kept a deleted conversation and never showed new ones.
 */
test('a reader leaving does not stop the updates the other readers still need', async () => {
  let push: ((value: unknown) => void) | undefined;
  const unsubscribe = vi.fn();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        onWorkspaceSessions: vi.fn((listener: (value: unknown) => void) => {
          push = listener;
          return unsubscribe;
        }),
        getWorkspaceSessions: vi.fn().mockResolvedValue({ ok: true, value: change('1', ['甲']) }),
      },
    },
  });
  const sidebar = render(<Titles label="sidebar" />);
  const archive = render(<Titles label="archive" />);
  await waitFor(() => expect(screen.getByLabelText('sidebar').textContent).toBe('甲'));

  archive.unmount();
  expect(unsubscribe).not.toHaveBeenCalled();
  act(() => push?.(change('2', ['甲', '乙'])));
  expect(screen.getByLabelText('sidebar').textContent).toBe('甲,乙');

  sidebar.unmount();
  expect(unsubscribe).toHaveBeenCalledTimes(1);
});
