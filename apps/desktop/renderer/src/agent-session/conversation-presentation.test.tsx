import type {
  AgentSessionProjection,
  ToolCallActivitySummaryView,
} from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';
import { AgentToolActivityCard } from '../AgentToolActivityCard';
import { ToolCallChip } from './ToolCallChip';
import { openProcesses } from './process-test-helpers';

const activity = vi.fn();
const readResult = vi.fn();

/** One page of real Tool activity for the visible call, in a chosen state. */
const activityPage = (status: string, sideEffectState: string | null = null) => ({
  ok: true,
  value: {
    session_id: 'session',
    next_before_call_id: null,
    calls: [
      {
        model_step_id: 'step',
        display: {
          title: '读取文件',
          target: 'note.txt',
          summary: '文件共 2 行',
          details: [],
          truncated: false,
          internal: false,
        },
        summary: {
          tool_call_id: 'tcl_01HOSTTOOLCALL0000000001',
          tool_batch_id: 'tbt_1',
          ordinal: 1,
          tool_id: 'tol_01',
          tool_version: { definition_revision: '1', semantic_version: null },
          display_name: 'fs.read_text',
          status,
          outcome: status === 'finalized' ? { type: 'succeeded' } : null,
          side_effect_state: sideEffectState,
          started_at: null,
          completed_at: null,
          can_cancel: false,
        },
        shell_query_available: false,
        permission_mode: 'read_only',
        permission_revision: 1,
      },
    ],
  },
});

beforeEach(() => {
  activity.mockReset().mockResolvedValue(activityPage('finalized'));
  readResult
    .mockReset()
    .mockResolvedValue({ ok: true, value: { text: 'file body', next_offset: null } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentToolActivity: activity, readAgentResult: readResult } },
  });
});
afterEach(cleanup);

test.each(['succeeded', 'operation_rejected'] as const)(
  'unknown effect overrides the %s icon in the detail chip as well as its label',
  (outcome) => {
    const summary = activityPage('finalized', 'unknown').value.calls[0]!.summary;
    render(
      <ToolCallChip
        call={{ ...summary, outcome: { type: outcome } } as unknown as ToolCallActivitySummaryView}
        callId={summary.tool_call_id}
      />,
    );
    expect(screen.getByText('结果未知')).toBeDefined();
    expect(document.querySelector('.tool-chip-unknown')).not.toBeNull();
    expect(document.querySelector('.tool-chip-success, .tool-chip-failed')).toBeNull();
    expect(document.querySelector('.tool-chip-dot[data-icon="check"]')).toBeNull();
  },
);

const projection = (
  state: string,
  partial: boolean,
  revision = '2',
  finalTextSource?: AgentSessionProjection['turns'][number]['final_text_source'],
  toolCallIds: string[] = ['tcl_01HOSTTOOLCALL0000000001'],
): AgentSessionProjection =>
  ({
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: state === 'running' ? { run: { state: { state: 'running' } } } : null,
    turns: [
      {
        turn: {
          mode: 'normal',
          state: state === 'completed' ? { state, final_report: 'Final response' } : { state },
          session_id: 'session',
          turn_task_id: 'turn',
          revision,
          root_user_event_id: 'root',
          created_at: '2026-09-23T00:00:00Z',
          updated_at: '2026-09-23T00:00:05Z',
        },
        final_text_source: finalTextSource,
        user_messages: [],
        assistant_messages: [
          {
            model_step_id: 'step',
            model_profile_id: null,
            context_snapshot_id: 'mex_1',
            text: 'answer',
            reasoning: 'because',
            text_total_bytes: 6,
            reasoning_total_bytes: 7,
            partial,
            stream_sequence: null,
            tool_call_ids: toolCallIds,
            at: '2026-09-23T00:00:00Z',
          },
        ],
        goal: null,
      },
    ],
  }) as unknown as AgentSessionProjection;

function show(state: string, partial: boolean) {
  render(
    <Conversation
      projection={projection(state, partial)}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={vi.fn()}
    />,
  );
}

const props = {
  viewportRef: { current: null },
  onChanged: vi.fn(),
  loading: false,
  error: undefined,
  loadOlder: vi.fn(),
  loadTurnOlder: vi.fn(),
};

test('completed turn separates final response, compact facts and inspection', async () => {
  show('completed', false);
  await openProcesses();
  const title = () => screen.getByText('读取文件', { selector: '.agent-tool-title' });
  await waitFor(() => expect(title()).toBeDefined());
  // The final answer renders as Markdown inside the final-report container.
  const final = document.querySelector('.agent-final-report');
  expect(final?.textContent).toBe('Final response');
  expect(final?.classList.contains('markdown-body')).toBe(true);
  // The Markdown renderer produces real block elements for a paragraph.
  expect(final?.querySelector('p')).not.toBeNull();
  // The closed sections name what is in them (the step's text stands between its
  // reasoning and its call); the audit waits behind the status line.
  expect(screen.getByRole('button', { name: '思考' })).toBeTruthy();
  const toggle = screen.getByRole('button', { name: '读取文件' });
  expect(toggle.getAttribute('aria-expanded')).toBe('false');
  fireEvent.click(toggle);
  expect(title().closest('.agent-tool-row')?.textContent).toContain('note.txt');
  expect(screen.getByText('文件共 2 行')).toBeDefined();
  expect(document.querySelector('.agent-tool-activity .tool-chip')).toBeNull();
  expect(document.querySelector('.agent-turn-audit')).toBeNull();
  expect(document.querySelectorAll('.agent-turn-status')).toHaveLength(1);
  expect(screen.queryByText('Aura')).toBeNull();
});

test('reasoning keeps its readable body and keyboard-native compact disclosure', () => {
  show('running', true);
  expect(screen.getByText('思考过程').closest('details')?.className).toBe('agent-reasoning-trace');
  expect(document.querySelector('.agent-reasoning')?.textContent).toBe('because');
});

test('unknown side effects never appear as a successful action', async () => {
  activity.mockResolvedValue(activityPage('finalized', 'unknown'));
  show('completed', false);
  await openProcesses();
  await waitFor(() =>
    expect(document.querySelector('.agent-tool-state')?.textContent?.trim()).toBe('结果未知'),
  );
  // The icon tone carries the same real state as the text; colour is never alone.
  expect(document.querySelector('.agent-tool-state')?.getAttribute('data-tone')).toBe('unknown');
  expect(document.querySelector('.agent-tool-activity')?.getAttribute('data-outcome')).toBe(
    'indeterminate',
  );
});

test('a queued call refreshes when the turn advances, terminal facts stay cached', async () => {
  activity.mockReset().mockResolvedValueOnce(activityPage('queued'));
  const view = render(<Conversation {...props} projection={projection('running', true)} />);
  await waitFor(() =>
    expect(document.querySelector('.agent-tool-state')?.textContent).toBe('已排队'),
  );
  activity.mockResolvedValue(activityPage('finalized'));
  view.rerender(<Conversation {...props} projection={projection('completed', false, '3')} />);
  await openProcesses();
  await waitFor(() =>
    expect(document.querySelector('.agent-tool-state')?.textContent).toBe('成功'),
  );
  expect(activity).toHaveBeenCalledTimes(2);
  view.rerender(<Conversation {...props} projection={projection('completed', false, '4')} />);
  expect(activity).toHaveBeenCalledTimes(2);
});

test('one current status remains when historical steps exist; manual collapse wins', async () => {
  const initial = projection('running', true);
  const turn = initial.turns[0]!;
  turn.assistant_messages.unshift({
    ...turn.assistant_messages[0]!,
    model_step_id: 'old',
    partial: false,
    tool_call_ids: [],
    text: 'old result',
  });
  const view = render(<Conversation {...props} projection={initial} />);
  await waitFor(() =>
    expect(document.querySelector('.agent-turn-status [role="status"]')?.textContent).toBe(
      '正在生成',
    ),
  );
  // A thin turning arc, not the square of dots.
  expect(document.querySelectorAll('.agent-turn-spinner')).toHaveLength(1);
  expect(document.querySelector('.agent-orbit')).toBeNull();
  // The running turn's newest section is open; closing it by hand wins over later updates.
  const toggle = [...document.querySelectorAll('.agent-process-section > button')].at(-1)!;
  expect(toggle.getAttribute('aria-expanded')).toBe('true');
  fireEvent.click(toggle);
  expect(toggle.getAttribute('aria-expanded')).toBe('false');
  view.rerender(<Conversation {...props} projection={projection('running', true, '5')} />);
  expect(toggle.getAttribute('aria-expanded')).toBe('false');
  view.rerender(<Conversation {...props} projection={projection('cancelled', false, '6')} />);
  expect(document.querySelector('.agent-turn-status [role="status"]')?.textContent).toBe('已停止');
  expect(document.querySelector('.agent-turn-spinner')).toBeNull();
});

test('visible calls beyond 20 are looked up by identity in bounded batches', async () => {
  const value = projection('completed', false);
  const ids = Array.from({ length: 125 }, (_, index) => `call-${index}`);
  value.turns[0]!.assistant_messages[0]!.tool_call_ids = ids;
  activity.mockImplementation(async (request: { call_ids: string[] }) => ({
    ok: true,
    value: {
      session_id: 'session',
      next_before_call_id: null,
      calls: request.call_ids.map((id) => {
        const entry = activityPage('finalized').value.calls[0]!;
        return {
          ...entry,
          summary: { ...entry.summary, tool_call_id: id },
          display: { ...entry.display, target: id },
        };
      }),
    },
  }));
  render(<Conversation {...props} projection={value} />);
  await openProcesses();
  await waitFor(() => expect(document.querySelectorAll('.agent-tool-activity')).toHaveLength(125));
  expect(activity.mock.calls.map(([request]) => request.call_ids.length)).toEqual([100, 25]);
  expect(document.querySelector('.agent-file-label[title="call-0"]')).not.toBeNull();
  expect(document.querySelector('.agent-file-label[title="call-124"]')).not.toBeNull();
});

test('process call counts exclude the surrounding session activity card', async () => {
  const value = projection('completed', false);
  const ids = Array.from({ length: 26 }, (_, index) => `call-${index}`);
  value.turns[0]!.assistant_messages[0]!.tool_call_ids = ids;
  activity.mockImplementation(async (request: { call_ids?: string[] }) => {
    const page = activityPage('finalized');
    const entry = page.value.calls[0]!;
    return {
      ...page,
      value: {
        ...page.value,
        calls: (request.call_ids ?? [ids[0]!]).map((id) => ({
          ...entry,
          summary: { ...entry.summary, tool_call_id: id },
        })),
      },
    };
  });
  render(
    <>
      <AgentToolActivityCard sessionId="session" />
      <Conversation {...props} projection={value} />
    </>,
  );
  await screen.findByRole('region', { name: '当前工具活动' });
  await openProcesses();
  await waitFor(() =>
    expect(
      document.querySelectorAll('[data-turn-id="turn"] .agent-steps .agent-tool-activity'),
    ).toHaveLength(26),
  );
  expect(document.querySelectorAll('.agent-tool-activity')).toHaveLength(27);
  expect(document.querySelectorAll('[data-turn-id="turn"] .agent-tool-pending')).toHaveLength(0);
});

test('a natural final answer renders once, from the step that produced it', async () => {
  // The projection names the answering step, so the text appears in the final
  // area only — even though the persisted report string differs from it.
  render(
    <Conversation
      {...props}
      projection={projection('completed', false, '2', {
        kind: 'assistant_step',
        model_step_id: 'step',
      })}
    />,
  );
  await openProcesses();
  await waitFor(() => expect(document.querySelector('.agent-final-report')).not.toBeNull());
  expect(document.querySelector('.agent-final-report')?.textContent).toBe('answer');
  expect(document.querySelectorAll('.agent-interim-reply')).toHaveLength(0);
  expect(screen.queryByText('Final response')).toBeNull();
  // Reasoning and the Tool call of the same step stay in the process trace.
  expect(document.querySelector('.agent-reasoning')?.textContent).toBe('because');
  expect(
    document.querySelectorAll('[data-turn-id="turn"] .agent-steps .agent-tool-activity'),
  ).toHaveLength(1);
});

test('a completion report still renders once for tool-based completion', async () => {
  render(
    <Conversation
      {...props}
      projection={projection('completed', false, '2', { kind: 'completion_report' })}
    />,
  );
  await openProcesses();
  await waitFor(() =>
    expect(document.querySelector('.agent-final-report')?.textContent).toBe('Final response'),
  );
  // This step's text is a reply of its own here: the report is not duplicated.
  expect(document.querySelectorAll('.agent-interim-reply')).toHaveLength(1);
});

test('a user message renders Markdown, while reasoning stays plain text', async () => {
  const value = projection('running', false);
  value.turns[0]!.turn.root_user_event_id = 'root';
  value.turns[0]!.user_messages = [
    {
      event_id: 'root',
      at: '2026-09-23T00:00:00Z',
      origin: { origin: 'user' },
      text: '**请看** `config.ts`\n\n- 第一项\n- 第二项',
      total_bytes: 40,
    },
  ] as unknown as (typeof value.turns)[number]['user_messages'];
  render(<Conversation {...props} projection={value} />);
  const message = await screen.findByRole('article');
  // Real elements, not one flat paragraph. The renderer marks the elements it
  // styles, so emphasis is matched through its own marker.
  expect(message.querySelector('[data-streamdown="strong"]')?.textContent).toBe('请看');
  expect(message.querySelector('[data-streamdown="inline-code"]')?.textContent).toBe('config.ts');
  expect(message.querySelectorAll('ul li')).toHaveLength(2);
  // Reasoning of the same turn stays a plain paragraph.
  expect(document.querySelector('.agent-reasoning')?.tagName).toBe('P');
  expect(document.querySelector('.agent-reasoning')?.textContent).toBe('because');
});

test('a historical turn without a completion source stays readable', async () => {
  render(<Conversation {...props} projection={projection('completed', false)} />);
  await openProcesses();
  await waitFor(() =>
    expect(document.querySelector('.agent-final-report')?.textContent).toBe('Final response'),
  );
  expect(document.querySelectorAll('.agent-interim-reply')).toHaveLength(1);
});

test('consecutive identical successful calls collapse without losing any call', async () => {
  const ids = [
    'tcl_01HOSTTOOLCALL0000000001',
    'tcl_01HOSTTOOLCALL0000000002',
    'tcl_01HOSTTOOLCALL0000000003',
  ];
  activity.mockImplementation(async () => {
    const page = activityPage('finalized');
    const entry = page.value.calls[0]!;
    return {
      ...page,
      value: {
        ...page.value,
        calls: ids.map((id, index) => ({
          ...entry,
          display: { ...entry.display, target: `src/dir-${index}` },
          summary: { ...entry.summary, tool_call_id: id },
        })),
      },
    };
  });
  render(
    <Conversation {...props} projection={projection('completed', false, '2', undefined, ids)} />,
  );
  await openProcesses();
  await waitFor(() => expect(document.querySelectorAll('.agent-tool-group')).toHaveLength(1));
  const group = document.querySelector('.agent-tool-group');
  expect(group?.querySelector('.agent-tool-title')?.textContent).toBe('读取文件');
  expect(group?.querySelector('.agent-tool-state')?.textContent).toContain('3 次');
  // Expansion keeps every call with its own identity and target.
  expect(group?.querySelectorAll('.agent-tool-group-members .agent-tool-activity')).toHaveLength(3);
  expect(
    [...(group?.querySelectorAll('.agent-file-label') ?? [])].map((node) => node.textContent),
  ).toEqual(['src/dir-0', 'src/dir-1', 'src/dir-2']);
  // The real call count never becomes the group count.
  expect(screen.getByRole('button', { name: /3 次工具调用/u })).toBeTruthy();
});
