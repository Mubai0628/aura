import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';
import { openProcesses } from './process-test-helpers';
import { forgetToolCallDirectories } from './useToolCallDirectory';

/**
 * The process view: what the model writes between its tools is a reply in the
 * conversation, not a line inside the collapsed process; the process itself is split around those
 * replies, and its audit waits behind the status line.
 */
const activity = vi.fn();

const call = (id: string, title: string, outcome: string) => ({
  model_step_id: 'step-1',
  display: { title, target: null, summary: '', details: [], truncated: false, internal: false },
  summary: {
    tool_call_id: id,
    tool_batch_id: 'tbt_1',
    ordinal: 1,
    tool_id: 'tol_01',
    tool_version: { definition_revision: '1', semantic_version: null },
    display_name: title === '读取网页' ? 'web.fetch' : 'shell.powershell',
    status: 'finalized',
    outcome: { type: outcome },
    side_effect_state: null,
    started_at: null,
    completed_at: null,
    can_cancel: false,
  },
  shell_query_available: false,
  permission_mode: 'read_only',
  permission_revision: 1,
});

beforeEach(() => {
  activity.mockReset().mockResolvedValue({
    ok: true,
    value: {
      session_id: 'session',
      next_before_call_id: null,
      calls: [
        call('tcl_01HOSTTOOLCALL0000000001', '读取网页', 'succeeded'),
        call('tcl_01HOSTTOOLCALL0000000002', '运行命令', 'succeeded'),
      ],
    },
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentToolActivity: activity,
        readAgentResult: vi
          .fn()
          .mockResolvedValue({ ok: true, value: { text: '', next_offset: null } }),
      },
    },
  });
});
afterEach(cleanup);

const step = (id: string, at: string, extra: Record<string, unknown>) => ({
  model_step_id: id,
  model_profile_id: null,
  context_snapshot_id: `mex_${id}`,
  text: '',
  reasoning: '',
  text_total_bytes: 0,
  reasoning_total_bytes: 0,
  partial: false,
  stream_sequence: null,
  tool_call_ids: [],
  at,
  ...extra,
});

/** A typical turn: think, say what comes next, run two tools, then answer. */
function turn(state: 'running' | 'completed', steer = false): AgentSessionProjection {
  const interim = 'Retrying now — both the built-in web tool and the direct HTTP request.';
  return {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: state === 'running' ? { run: { state: { state: 'running' } } } : null,
    turns: [
      {
        turn: {
          mode: 'normal',
          state: state === 'completed' ? { state, final_report: 'Both worked.' } : { state },
          session_id: 'session',
          turn_task_id: 'turn',
          revision: '3',
          root_user_event_id: 'root',
          created_at: '2026-10-18T00:00:00Z',
          updated_at: '2026-10-18T00:00:10Z',
        },
        final_text_source:
          state === 'completed' ? { kind: 'assistant_step', model_step_id: 'step-2' } : null,
        user_messages: steer
          ? [
              {
                event_id: 'steer',
                at: '2026-10-18T00:00:02Z',
                origin: { origin: 'steer' },
                text: '顺便看看代理设置',
                total_bytes: 24,
              },
            ]
          : [],
        assistant_messages: [
          step('step-1', '2026-10-18T00:00:01Z', {
            reasoning: 'The gateway failed last time; try both paths.',
            reasoning_total_bytes: 46,
            text: interim,
            text_total_bytes: interim.length,
            tool_call_ids: ['tcl_01HOSTTOOLCALL0000000001', 'tcl_01HOSTTOOLCALL0000000002'],
          }),
          step('step-2', '2026-10-18T00:00:05Z', {
            reasoning: 'Both answered.',
            reasoning_total_bytes: 14,
            text: 'Both worked.',
            text_total_bytes: 12,
            partial: state === 'running',
          }),
        ],
        goal: null,
      },
    ],
  } as unknown as AgentSessionProjection;
}

const props = {
  viewportRef: { current: null },
  onChanged: vi.fn(),
  loading: false,
  error: undefined,
  loadOlder: vi.fn(),
  loadTurnOlder: vi.fn(),
};

test('what the model writes between its tools reads as a reply, between the process sections', async () => {
  render(<Conversation {...props} projection={turn('completed')} />);
  await openProcesses();
  const reply = await screen.findByText(/Retrying now/u);
  expect(reply.closest('.agent-interim-reply')).not.toBeNull();
  expect(reply.closest('.agent-steps, .agent-process-section')).toBeNull();
  // Order on the page: the first thought, the reply, then the tools and the next thought, then
  // the answer.
  const turnElement = document.querySelector('[data-turn-id="turn"]')!;
  const blocks = [
    ...turnElement.querySelectorAll(
      '.agent-process-section, .agent-interim-reply, .agent-final-report',
    ),
  ].map((block) => block.className.split(' ')[0]);
  expect(blocks).toEqual([
    'agent-process-section',
    'agent-interim-reply',
    'agent-process-section',
    'agent-final-report',
  ]);
  expect(document.querySelector('.agent-final-report')?.textContent).toBe('Both worked.');
  // Each section says what is in it while it is closed.
  await waitFor(() =>
    expect(screen.getByRole('button', { name: /读取网页、运行命令/u })).toBeTruthy(),
  );
  expect(screen.getAllByRole('button', { name: /^思考/u })).toHaveLength(2);
});

// "过程部分结束后自动折叠起来，也能手动打开和折叠".
test('a finished turn folds its process behind its status line, which opens and closes it', async () => {
  render(<Conversation {...props} projection={turn('completed')} />);
  const status = await screen.findByRole('button', { name: /^已完成/u });
  expect(status.getAttribute('aria-expanded')).toBe('false');
  // Folded: no section and no reply between the tools; the answer stays.
  expect(document.querySelector('.agent-process-section')).toBeNull();
  expect(screen.queryByText(/Retrying now/u)).toBeNull();
  expect(document.querySelector('.agent-final-report')?.textContent).toBe('Both worked.');
  fireEvent.click(status);
  expect(status.getAttribute('aria-expanded')).toBe('true');
  await screen.findByText(/Retrying now/u);
  fireEvent.click(status);
  expect(screen.queryByText(/Retrying now/u)).toBeNull();
  expect(document.querySelector('.agent-process-section')).toBeNull();
});

// What the user opened in a turn is still open when the view comes back.
test('a turn opened before leaving the conversation is still open on coming back', async () => {
  const first = render(<Conversation {...props} projection={turn('completed')} />);
  fireEvent.click(await screen.findByRole('button', { name: /^已完成/u }));
  await screen.findByText(/Retrying now/u);
  first.unmount();
  render(<Conversation {...props} projection={turn('completed')} />);
  expect(screen.getByRole('button', { name: /^已完成/u }).getAttribute('aria-expanded')).toBe(
    'true',
  );
  expect(screen.getByText(/Retrying now/u)).not.toBeNull();
});

test('a running turn shows its process and folds it once the turn has completed', async () => {
  const view = render(<Conversation {...props} projection={turn('running')} />);
  await screen.findByText(/Retrying now/u);
  expect(document.querySelector('.agent-turn-toggle')).toBeNull();
  view.rerender(<Conversation {...props} projection={turn('completed')} />);
  await waitFor(() => expect(screen.queryByText(/Retrying now/u)).toBeNull());
  expect(screen.getByRole('button', { name: /^已完成/u }).getAttribute('aria-expanded')).toBe(
    'false',
  );
});

// A turn has no step inspection.
test('a finished turn keeps its sections closed and offers no step inspection', async () => {
  render(<Conversation {...props} projection={turn('completed')} />);
  await openProcesses();
  await screen.findByText(/Retrying now/u);
  for (const toggle of document.querySelectorAll('.agent-process-section > button')) {
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
  }
  expect(screen.queryByRole('button', { name: '检查详情' })).toBeNull();
  expect(screen.queryByRole('button', { name: /检查步骤/u })).toBeNull();
});

test('while the turn runs only its newest section is open, and a manual choice is kept', async () => {
  const view = render(<Conversation {...props} projection={turn('running')} />);
  await screen.findByText(/Retrying now/u);
  const toggles = () => [...document.querySelectorAll('.agent-process-section > button')];
  await waitFor(() => expect(toggles()).toHaveLength(2));
  expect(toggles().map((toggle) => toggle.getAttribute('aria-expanded'))).toEqual([
    'false',
    'true',
  ]);
  fireEvent.click(toggles()[0]!);
  view.rerender(<Conversation {...props} projection={turn('running')} />);
  expect(toggles()[0]!.getAttribute('aria-expanded')).toBe('true');
});

test('a section with a failed call opens by itself', async () => {
  activity.mockResolvedValue({
    ok: true,
    value: {
      session_id: 'session',
      next_before_call_id: null,
      calls: [
        call('tcl_01HOSTTOOLCALL0000000001', '读取网页', 'failed'),
        call('tcl_01HOSTTOOLCALL0000000002', '运行命令', 'succeeded'),
      ],
    },
  });
  render(<Conversation {...props} projection={turn('completed')} />);
  await openProcesses();
  await waitFor(() =>
    expect(
      screen.getByRole('button', { name: /读取网页、运行命令/u }).getAttribute('aria-expanded'),
    ).toBe('true'),
  );
});

test('a message the user added during the turn reads as their message, not a process line', async () => {
  render(<Conversation {...props} projection={turn('completed', true)} />);
  const added = await screen.findByText('顺便看看代理设置');
  // The user's own words stay in view while the process is folded.
  expect(screen.getByRole('button', { name: /^已完成/u }).getAttribute('aria-expanded')).toBe(
    'false',
  );
  expect(added.closest('.agent-steer-message')).not.toBeNull();
  expect(added.closest('.agent-steps, .agent-process-section')).toBeNull();
});

// The button sits under the
// turn's status line, in view once the turn has folded its process, and shown once when it is open.
test('a call the debugging switch stopped keeps its enable button in view after the turn folds', async () => {
  // The same call ids as the tests above, read again with the refusal.
  forgetToolCallDirectories();
  const refused = call('tcl_01HOSTTOOLCALL0000000001', '检查网页元素', 'not_executed');
  activity.mockResolvedValue({
    ok: true,
    value: {
      session_id: 'session',
      next_before_call_id: null,
      calls: [
        { ...refused, display: { ...refused.display, offer: 'enable_browser_debugging' } },
        call('tcl_01HOSTTOOLCALL0000000002', '运行命令', 'succeeded'),
      ],
    },
  });
  render(<Conversation {...props} projection={turn('completed')} />);
  const status = await screen.findByRole('button', { name: /^已完成/u });
  expect(status.getAttribute('aria-expanded')).toBe('false');
  await screen.findByRole('button', { name: '允许模型调试网页' });
  fireEvent.click(status);
  await screen.findByText(/Retrying now/u);
  await waitFor(() =>
    expect(screen.getAllByRole('button', { name: '允许模型调试网页' })).toHaveLength(1),
  );
});
