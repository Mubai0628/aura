import type { AgentMessageAttachment, AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { resetPictureReadsForTests } from '../composer-attachments/picture-reads';
import { FileLinkContext } from '../files/file-links';
import { Conversation } from './Conversation';
import { openProcesses } from './process-test-helpers';
import { forgetTurnChoices } from './turn-choices';
import { forgetToolCallDirectories } from './useToolCallDirectory';

/**
 * The process view: what the model writes between its tools is a reply in the
 * conversation, not a line inside the collapsed process; the process itself is split around those
 * replies, and its audit waits behind the status line.
 */
const activity = vi.fn();
const readPicture = vi.fn();

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
  resetPictureReadsForTests();
  readPicture.mockReset().mockResolvedValue({
    ok: true,
    value: { media_type: 'image/png', data_base64: 'iVBORw0KGgo=' },
  });
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
        browser: { readScreenshot: readPicture },
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

test('an adopted attachment-only addition retains one thumbnail, one file card and their menus', async () => {
  const id = 'steer-with-attachments';
  const at = '2026-10-18T00:00:02Z';
  const attachments: AgentMessageAttachment[] = [
    {
      kind: 'image',
      name: 'sample.png',
      media_type: 'image/png',
      byte_length: 8,
      artifact_id: 'art_image',
    },
    { kind: 'text', name: 'sample.txt', byte_length: 9, artifact_id: 'art_text' },
  ];
  const pending: AgentSessionProjection = {
    ...turn('running'),
    inbox: {
      session_id: 'session',
      revision: '1',
      paused: false,
      messages: [
        {
          message_id: id,
          created_at: at,
          text: '',
          destination: { lane: 'addition', turn_task_id: 'turn' },
          attachments,
        },
      ],
    },
  };
  const show = (projection: AgentSessionProjection) => (
    <FileLinkContext.Provider
      value={{ session: 'session', workspace: 'workspace', root: 'D:\\Fixture', notify: vi.fn() }}
    >
      <Conversation {...props} projection={projection} />
    </FileLinkContext.Provider>
  );
  const view = render(show(pending));
  const picture = await screen.findByRole('img', { name: 'sample.png' });
  expect(picture.closest('article')?.getAttribute('aria-label')).toBe('追加信息');
  expect(screen.getByRole('button', { name: '打开 sample.txt' })).toBeTruthy();

  const adopted = {
    ...pending,
    turns: [
      {
        ...pending.turns[0]!,
        user_messages: [
          {
            event_id: id,
            at,
            origin: { origin: 'steer' as const },
            text: '',
            total_bytes: 0,
            attachments,
          },
        ],
      },
    ],
  };
  // The history can already contain the addition before the inbox projection catches up.
  view.rerender(show(adopted));
  expect(screen.getAllByRole('img', { name: 'sample.png' })).toHaveLength(1);
  expect(screen.queryByRole('status', { name: '等待模型接收' })).toBeNull();
  expect(readPicture).toHaveBeenCalledTimes(1);
  fireEvent.contextMenu(screen.getByRole('button', { name: '打开 sample.txt' }));
  expect(
    within(screen.getByRole('menu')).getByRole('menuitem', { name: '在侧边栏打开' }),
  ).toBeTruthy();
  fireEvent.contextMenu(screen.getByRole('button', { name: '预览 sample.png' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '预览' }));
  expect(screen.getByRole('dialog', { name: '图片预览' })).toBeTruthy();
});

test('what the model writes between its tools reads as a reply, between the process sections', async () => {
  render(<Conversation {...props} projection={turn('completed')} />);
  await openProcesses();
  const reply = await screen.findByText(/Retrying now/u);
  expect(reply.closest('.agent-interim-reply')).not.toBeNull();
  expect(reply.closest('.agent-interim-reply')?.getAttribute('aria-label')).toBe('过程回复');
  expect(screen.queryByText('过程回复')).toBeNull();
  expect(reply.closest('.agent-interim-reply')?.classList.contains('is-current')).toBe(false);
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
  expect(document.querySelector('.agent-final-report .agent-interim-label')).toBeNull();
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

test('an older refused debugging call has no settings action in a completed turn', async () => {
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
  fireEvent.click(status);
  await screen.findByText(/Retrying now/u);
  await screen.findAllByText('检查网页元素');
  expect(screen.queryByRole('button', { name: '允许模型调试网页' })).toBeNull();
});

/** The same turn once its first step, read when the reader reached it, is in front of the rest. */
function withEarlierStep(projection: AgentSessionProjection): AgentSessionProjection {
  const [only] = projection.turns;
  const first = step('step-0', '2026-10-18T00:00:00Z', {
    reasoning: 'Read the task first.',
    reasoning_total_bytes: 20,
  });
  return {
    ...projection,
    turns: [{ ...only!, assistant_messages: [first, ...only!.assistant_messages] }],
  } as unknown as AgentSessionProjection;
}

// The earlier steps of a long turn are read when the reader reaches them, and the first of them
// joins the section that was first.
test('a section opened by hand stays as chosen when the turn’s earlier steps arrive in front of it', async () => {
  forgetTurnChoices();
  const view = render(<Conversation {...props} projection={turn('completed')} />);
  await openProcesses();
  await screen.findByText(/Retrying now/u);
  const first = () => document.querySelector('.agent-process-section > button')!;
  expect(first().getAttribute('aria-expanded')).toBe('false');
  fireEvent.click(first());
  view.rerender(<Conversation {...props} projection={withEarlierStep(turn('completed'))} />);
  expect(first().getAttribute('aria-expanded')).toBe('true');
  expect(first().closest('.agent-process-section')!.textContent).toContain('Read the task first.');
  fireEvent.click(first());
  view.rerender(<Conversation {...props} projection={withEarlierStep(turn('completed'))} />);
  expect(first().getAttribute('aria-expanded')).toBe('false');
});

test('a delivered interim message appears once after the user addition and stays visible when the turn folds', async () => {
  const projection = turn('completed', true);
  const step = projection.turns[0]!.assistant_messages[0]!;
  const callId = 'tcl_01HOSTTOOLCALL0000000003';
  Object.assign(step, {
    tool_call_ids: [callId],
    interim_replies: [
      { tool_call_id: callId, text: '第一阶段已完成，继续核对。', at: '2026-10-18T00:00:03Z' },
    ],
  });
  activity.mockResolvedValue({
    ok: true,
    value: {
      session_id: 'session',
      next_before_call_id: null,
      calls: [call(callId, '中途回复', 'succeeded')],
    },
  });
  render(<Conversation {...props} projection={projection} />);
  const reply = await screen.findByText('第一阶段已完成，继续核对。');
  expect(reply.closest('.agent-interim-reply')).toBeTruthy();
  expect(reply.closest('.agent-interim-reply')?.getAttribute('aria-label')).toBe('过程回复');
  expect(screen.queryByText('过程回复')).toBeNull();
  expect(screen.getAllByText('第一阶段已完成，继续核对。')).toHaveLength(1);
  const user = screen.getByText('顺便看看代理设置');
  const answer = screen.getByText('Both worked.');
  expect(user.compareDocumentPosition(reply) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  expect(reply.compareDocumentPosition(answer) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  await openProcesses();
  expect(screen.queryByText('中途回复')).toBeNull();
});
