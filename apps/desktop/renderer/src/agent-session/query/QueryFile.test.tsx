import type { UserQuery } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { QueryFile } from './QueryFile';

// A file chosen as an answer that was not taken says why, that the answer has not gone, and
// what to do.

const query = {
  query_id: 'qry_1',
  agent_run_id: 'run_1',
  revision: '1',
} as unknown as UserQuery;
const select = vi.fn();
beforeEach(() => {
  select.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { selectAgentQueryFile: select } },
  });
});
afterEach(cleanup);

async function choose(path: string) {
  const user = userEvent.setup();
  await user.type(screen.getByPlaceholderText('相对工作区的文件或目录'), path);
  await user.click(screen.getByRole('button', { name: '确认文件或目录' }));
}

test('a file that was not taken says why and that the answer has not gone', async () => {
  select.mockResolvedValueOnce({
    ok: false,
    error: { code: 'ipc.request_timeout', message: 'Core request timed out' },
  });
  render(<QueryFile sessionId="s" query={query} questionId="q" onSelected={vi.fn()} />);
  await choose('docs/a.md');
  expect(screen.getByRole('status').textContent).toBe(
    '这个文件没有选上（Aura Core 没有及时回复），回答还没有发出；请重新选择。',
  );
});

test('a file outside the workspace says to pick one inside it', async () => {
  select.mockResolvedValueOnce({
    ok: false,
    error: { code: 'agent.query_file_scope', message: 'agent.query_file_scope' },
  });
  render(<QueryFile sessionId="s" query={query} questionId="q" onSelected={vi.fn()} />);
  await choose('../outside.md');
  expect(screen.getByRole('status').textContent).toBe(
    '这个文件不在工作区里，不能用来回答；请选工作区里的文件或目录。',
  );
});

test('a choice whose answer did not come says so', async () => {
  select.mockRejectedValueOnce(new Error('Aura Core pipe disconnected'));
  render(<QueryFile sessionId="s" query={query} questionId="q" onSelected={vi.fn()} />);
  await choose('docs/a.md');
  expect(screen.getByRole('status').textContent).toBe(
    '这个文件没有选上（与 Aura Core 的连接断开了），回答还没有发出；请重新选择。',
  );
});
