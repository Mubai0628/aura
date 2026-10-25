import type { AgentSessionProjection } from '@aura/contracts/generated';
import { cleanup, render, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { Conversation } from './Conversation';
import { openProcesses } from './process-test-helpers';

// "任务过程中的回复和复制按钮占用的上下距离太大了……将消息的快捷操作只设计在模型
// 最终的回复和用户输入的消息上"; "将消息的快捷操作按钮都放到AI最终回复下方的直线的下面、间距紧凑一些，且
// 用户输入也要有快捷复制操作".

const texts: Record<string, string> = {
  'user:root': '写一个2D的雷霆战机小游戏HTML页面',
  'assistant:note': '我先看一下工作区结构，然后开始写。',
  'assistant:answer': '打开方式：双击 thunder-fighter.html。',
};
const writeClipboard = vi.fn();
beforeEach(() => {
  writeClipboard
    .mockReset()
    .mockResolvedValue({ ok: true, value: { completed: true, message: '已复制' } });
  const readAgentMessageText = vi.fn(
    async (request: { source: { kind: string; event_id?: string; model_step_id?: string } }) => {
      const key = `${request.source.kind}:${request.source.event_id ?? request.source.model_step_id}`;
      const text = texts[key]!;
      const bytes = new TextEncoder().encode(text).length;
      return {
        ok: true,
        value: {
          source: request.source,
          text,
          total_bytes: bytes,
          from_offset: 0,
          next_offset: null,
          body_id: null,
        },
      };
    },
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentMessageText,
        writeClipboard,
        readAgentToolActivity: vi.fn(async () => ({
          ok: true,
          value: { session_id: 'session', next_before_call_id: null, calls: [] },
        })),
      },
    },
  });
});
afterEach(cleanup);

const bytes = (text: string) => new TextEncoder().encode(text).length;
const step = (id: string, text: string, toolCallIds: string[]) => ({
  model_step_id: id,
  model_profile_id: null,
  context_snapshot_id: 'mex_1',
  text,
  reasoning: '',
  text_total_bytes: bytes(text),
  reasoning_total_bytes: 0,
  partial: false,
  stream_sequence: null,
  tool_call_ids: toolCallIds,
  at: '2026-10-20T00:00:00Z',
});
const projection = {
  session_id: 'session',
  runtime_instance_id: 'runtime',
  active: null,
  turns: [
    {
      turn: {
        mode: 'normal',
        state: { state: 'completed', final_report: texts['assistant:answer'] },
        session_id: 'session',
        turn_task_id: 'turn',
        revision: '3',
        root_user_event_id: 'root',
        created_at: '2026-10-20T00:00:00Z',
        updated_at: '2026-10-20T00:00:09Z',
      },
      final_text_source: { kind: 'assistant_step', model_step_id: 'answer' },
      user_messages: [
        {
          event_id: 'root',
          text: texts['user:root'],
          total_bytes: bytes(texts['user:root']!),
          origin: { origin: 'submit' },
        },
      ],
      assistant_messages: [
        step('note', texts['assistant:note']!, ['tcl_01HOSTTOOLCALL0000000001']),
        step('answer', texts['assistant:answer']!, []),
      ],
      goal: null,
    },
  ],
} as unknown as AgentSessionProjection;

function show() {
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

test('the answer’s 复制 sits under the turn’s line, the last thing in it; notes between tools have none', async () => {
  show();
  await openProcesses();
  const turn = document.querySelector('.agent-turn') as HTMLElement;
  await waitFor(() => expect(turn.querySelector('.agent-interim-reply')).not.toBeNull());
  expect(
    within(turn.querySelector('.agent-interim-reply') as HTMLElement).queryByRole('button'),
  ).toBeNull();
  expect(
    within(turn.querySelector('.agent-final-report') as HTMLElement).queryByRole('button', {
      name: '复制',
    }),
  ).toBeNull();
  const footer = turn.querySelector(':scope > .agent-turn-footer') as HTMLElement;
  expect(footer).not.toBeNull();
  expect(turn.lastElementChild).toBe(footer);
  const user = userEvent.setup();
  await user.click(within(footer).getByRole('button', { name: '复制' }));
  await waitFor(() =>
    expect(writeClipboard).toHaveBeenCalledWith({ text: texts['assistant:answer'] }),
  );
});

test('the user’s message is copied whole through Main, from beside it', async () => {
  show();
  const message = document.querySelector('.agent-user-message') as HTMLElement;
  const actions = message.querySelector(':scope > .agent-user-actions') as HTMLElement;
  expect(actions).not.toBeNull();
  const user = userEvent.setup();
  await user.click(within(actions).getByRole('button', { name: '复制消息' }));
  await waitFor(() => expect(writeClipboard).toHaveBeenCalledWith({ text: texts['user:root'] }));
  expect(await within(actions).findByRole('button', { name: '已复制' })).toBeTruthy();
});
