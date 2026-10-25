import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { AgentToolActivityCard } from './AgentToolActivityCard';

const readAgentToolActivity = vi.fn();
const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const entry: AgentToolActivityEntry = {
  model_step_id: 'mst_00000000000000000000000001',
  display: {
    title: '读取文件',
    target: 'note.txt',
    summary: '未获授权',
    details: [],
    truncated: false,
    internal: false,
  },
  shell_query_available: false,
  summary: {
    can_cancel: false,
    completed_at: '2026-09-09T00:00:00Z',
    display_name: 'fs.read',
    ordinal: 0,
    outcome: { type: 'operation_rejected' },
    side_effect_state: 'no_effect',
    started_at: '2026-09-09T00:00:00Z',
    status: 'finalized',
    tool_batch_id: 'tbt_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    tool_call_id: 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    tool_version: { definition_revision: '1', semantic_version: '1.0.0' },
  },
};
beforeEach(() => {
  readAgentToolActivity.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentToolActivity } },
  });
});
afterEach(cleanup);
test('denial is a completed operation result; pagination uses the original session and switching does not retain its calls', async () => {
  readAgentToolActivity
    .mockResolvedValueOnce({
      ok: true,
      value: {
        session_id: session,
        calls: [entry],
        next_before_call_id: entry.summary.tool_call_id,
      },
    })
    .mockResolvedValueOnce({
      ok: true,
      value: { session_id: session, calls: [], next_before_call_id: null },
    })
    .mockResolvedValueOnce({
      ok: true,
      value: { session_id: 'other', calls: [], next_before_call_id: null },
    });
  const component = render(<AgentToolActivityCard sessionId={session} />);
  await screen.findByText('调用已完成 · 操作被拒绝');
  fireEvent.click(screen.getByRole('button', { name: '加载更早调用' }));
  expect(readAgentToolActivity.mock.calls[1]?.[0]).toEqual({
    session_id: session,
    before_call_id: entry.summary.tool_call_id,
    limit: 20,
  });
  component.rerender(<AgentToolActivityCard sessionId="other" />);
  expect(screen.queryByText('fs.read')).toBeNull();
});
