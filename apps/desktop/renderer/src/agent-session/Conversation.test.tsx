// eslint-disable-next-line no-restricted-imports -- a test reads its stylesheet: Vitest hands a CSS import over empty, and the rule is for the renderer's own code.
import { readFileSync } from 'node:fs';
import type { AgentSessionProjection } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';
import { acceptSending, resetSendingForTests, startSending } from './sending';

const related = vi.fn();
/** A controllable IntersectionObserver: the sentinel is only reached when a
 *  test says the viewport arrived at it. */
const observers: { callback: (entries: unknown[]) => void; element: Element }[] = [];
beforeEach(() => {
  related.mockReset().mockResolvedValue({ ok: true, value: {} });
  observers.length = 0;
  class FakeObserver {
    constructor(private readonly callback: (entries: unknown[]) => void) {
      this.callback = callback;
    }
    observe(element: Element): void {
      observers.push({ callback: this.callback, element });
    }
    disconnect(): void {
      /* nothing to release in the fake */
    }
  }
  Object.defineProperty(globalThis, 'IntersectionObserver', {
    configurable: true,
    value: FakeObserver,
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { startRelatedAgentTurn: related } },
  });
});
afterEach(() => {
  cleanup();
  resetSendingForTests();
});

function show(state: 'cancelled' | 'failed' | 'recovery_required' | 'completed', withPlan = false) {
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    turns: [
      {
        turn: {
          mode: withPlan ? 'plan' : 'normal',
          state:
            state === 'completed' ? { state: 'completed', final_report: '计划已完成' } : { state },
          session_id: 'session',
          turn_task_id: 'interrupted',
          revision: '4',
        },
        user_messages: [],
        assistant_messages: [],
        plan: withPlan
          ? {
              revision: '1',
              original_user_goal: '实现功能',
              plan: {
                current_project_state: '尚未实施',
                requirements: ['覆盖验收'],
                steps: [
                  {
                    id: 'one',
                    description: '编写实现',
                    validation: '测试通过',
                    depends_on: [],
                  },
                ],
              },
            }
          : null,
      },
    ],
  } as unknown as AgentSessionProjection;
  render(
    <Conversation
      projection={projection}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={vi.fn()}
    />,
  );
}

test.each(['cancelled', 'failed', 'recovery_required'] as const)(
  '%s turns have no continue entry',
  (state) => {
    show(state);
    expect(screen.queryByRole('button', { name: '核对历史并继续任务' })).toBeNull();
    expect(screen.queryByText('调整恢复后的总预算')).toBeNull();
    expect(screen.queryByText(/恢复来源/)).toBeNull();
  },
);

// The way back to the latest message took 50 px of the conversation's flow: it made the content
// taller just as the reader scrolled up from the end, which pulled them back to the end, where it
// went away again - the view flashed. It sits in a dock of no height that is always there.
test('offering the way back never changes how tall the conversation is', () => {
  show('completed');
  const conversation = screen.getByLabelText('会话消息');
  expect(conversation.querySelector(':scope > .agent-follow-dock')).not.toBeNull();
  // Vitest hands a CSS import over empty, so the stylesheet is read as a file.
  const css = readFileSync(`${__dirname}/agent-session.css`, 'utf8').replace(
    /\/\*[\s\S]*?\*\//gu,
    '',
  );
  const rule = (selector: string) =>
    css.match(new RegExp(`(?:^|\\})\\s*${selector}\\s*\\{([^}]*)\\}`, 'u'))?.[1] ?? '';
  expect(rule('\\.agent-follow-dock')).toMatch(/position: sticky;/u);
  expect(rule('\\.agent-follow-dock')).toMatch(/height: 0;/u);
  expect(rule('\\.agent-follow')).toMatch(/position: absolute;/u);
  expect(rule('\\.agent-follow')).not.toMatch(/margin/u);
});

test('completed plan keeps the implement entry', async () => {
  show('completed', true);
  fireEvent.click(screen.getByRole('button', { name: '实施计划' }));
  await waitFor(() => expect(related).toHaveBeenCalledTimes(1));
  expect(related).toHaveBeenCalledWith({
    session_id: 'session',
    source_turn_task_id: 'interrupted',
    expected_revision: '4',
    kind: 'implement_plan',
    goal_budget: null,
  });
});

test('reaching the top pages earlier turns with no button and no jump', async () => {
  const loadOlder = vi.fn().mockResolvedValue(true);
  const viewport = document.createElement('div');
  let height = 1000;
  Object.defineProperty(viewport, 'scrollHeight', { configurable: true, get: () => height });
  viewport.scrollTop = 500;
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    next_before_turn_task_id: 'turn-0',
    turns: [
      {
        turn: {
          session_id: 'session',
          turn_task_id: 'turn-1',
          revision: '1',
          state: { state: 'cancelled' },
        },
        user_messages: [],
        assistant_messages: [],
        plan: null,
      },
    ],
  } as unknown as AgentSessionProjection;
  render(
    <Conversation
      projection={projection}
      viewportRef={{ current: viewport }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={loadOlder}
      loadTurnOlder={vi.fn()}
    />,
  );
  // Earlier history is reached by scrolling, not by pressing anything.
  expect(screen.queryByRole('button', { name: '加载更早消息' })).toBeNull();
  expect(document.querySelector('.agent-history-sentinel')).not.toBeNull();
  const observer = observers.at(-1);
  expect(observer).toBeDefined();
  const before = viewport.scrollTop;

  loadOlder.mockImplementation(async () => {
    // The page lands above the reader: the content grows by 400px.
    height = 1400;
    return true;
  });
  await act(async () => {
    observer!.callback([{ isIntersecting: true, target: observer!.element }]);
  });
  expect(loadOlder).toHaveBeenCalledTimes(1);
  // The reader keeps looking at the same line: the added height is compensated.
  await waitFor(() => expect(viewport.scrollTop - before).toBe(400));
});

test('a turn with earlier messages pages them from its own marker', async () => {
  const loadTurnOlder = vi.fn().mockResolvedValue(true);
  const projection = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    next_before_turn_task_id: null,
    turns: [
      {
        turn: {
          session_id: 'session',
          turn_task_id: 'turn-1',
          revision: '1',
          state: { state: 'cancelled' },
        },
        user_messages: [],
        assistant_messages: [],
        plan: null,
        next_messages: {
          turn_task_id: 'turn-1',
          before_user_event_id: 'user-1',
          before_model_step_id: null,
        },
      },
    ],
  } as unknown as AgentSessionProjection;
  render(
    <Conversation
      projection={projection}
      viewportRef={{ current: null }}
      onChanged={vi.fn()}
      loading={false}
      error={undefined}
      loadOlder={vi.fn()}
      loadTurnOlder={loadTurnOlder}
    />,
  );
  expect(screen.queryByRole('button', { name: '加载此任务的更早消息' })).toBeNull();
  const observer = observers.at(-1);
  expect(observer?.element.closest('.agent-turn')).not.toBeNull();
  await act(async () => {
    observer!.callback([{ isIntersecting: true, target: observer!.element }]);
  });
  expect(loadTurnOlder).toHaveBeenCalledWith('turn-1');
});

// A message just sent shows at the end of its conversation, also while the conversation loads,
// and gives way to the real message once the projection has it.
test('a message being sent shows until the conversation has it', async () => {
  const viewportRef = { current: document.createElement('div') };
  const props = {
    sessionId: 'session',
    viewportRef,
    onChanged: vi.fn(),
    error: undefined,
    loadOlder: vi.fn(),
    loadTurnOlder: vi.fn(),
  };
  startSending('session', { text: '你好', files: [] });
  const { rerender } = render(<Conversation {...props} projection={undefined} loading />);
  expect(screen.getByText('发送中')).toBeTruthy();
  expect(screen.queryByText('正在加载会话…')).toBeNull();
  act(() => acceptSending('session'));
  const arrived = {
    session_id: 'session',
    runtime_instance_id: 'runtime',
    active: null,
    turns: [
      {
        turn: {
          mode: 'normal',
          state: { state: 'running' },
          session_id: 'session',
          turn_task_id: 'turn',
          revision: '1',
          root_user_event_id: 'event',
        },
        user_messages: [
          {
            event_id: 'event',
            text: '你好',
            total_bytes: 6,
            at: '2026-10-25T00:00:00Z',
            origin: 'user',
          },
        ],
        assistant_messages: [],
      },
    ],
  } as unknown as AgentSessionProjection;
  rerender(<Conversation {...props} projection={arrived} loading={false} />);
  await waitFor(() => expect(screen.queryByText('发送中')).toBeNull());
});
