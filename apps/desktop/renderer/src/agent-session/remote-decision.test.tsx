import type { AgentToolActivityEntry, AgentToolRemoteDecision } from '@aura/contracts/generated';
import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { remoteDecisionText } from './remote-decision';
import { ToolActivity } from './ToolActivity';

// A call whose approval was decided in a chat names the plugin, the platform and the person on
// its row; one decided in Aura's window says nothing more.
afterEach(cleanup);

const sender = (user_name: string, platform = 'qq'): AgentToolRemoteDecision['sender'] => ({
  plugin: 'chat-bot',
  from: { channel: 'qq-main', platform, chat: 'private', user_id: '8F2A', user_name },
});

const entry = (remote_decision?: AgentToolRemoteDecision): AgentToolActivityEntry =>
  ({
    display: {
      details: [],
      internal: false,
      summary: '',
      target: null,
      title: '写入文件',
      truncated: false,
    },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: 'fs.write',
      ordinal: 1,
      outcome: { type: 'succeeded' },
      side_effect_state: null,
      started_at: null,
      status: 'finalized',
      tool_batch_id: 'tbt_1',
      tool_call_id: 'tcl_01HOSTTOOLCALL0000000001',
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
    ...(remote_decision ? { remote_decision } : {}),
  }) as unknown as AgentToolActivityEntry;

test('who decided in a chat is named with the plugin, the platform and the decision', () => {
  expect(remoteDecisionText({ sender: sender('老王'), decision: 'allow_once' })).toBe(
    '经插件 chat-bot 由 QQ 用户 老王 批准（仅此一次）',
  );
  expect(remoteDecisionText({ sender: sender('', 'telegram'), decision: 'allow_session' })).toBe(
    '经插件 chat-bot 由 Telegram 用户 8F2A 批准（本会话）',
  );
  expect(remoteDecisionText({ sender: sender('老王'), decision: 'deny' })).toBe(
    '经插件 chat-bot 由 QQ 用户 老王 拒绝',
  );
  expect(remoteDecisionText({ sender: { plugin: 'chat-bot' }, decision: 'deny' })).toBe(
    '经插件 chat-bot 拒绝',
  );
});

test('a call decided in a chat says so on its row', () => {
  render(<ToolActivity entry={entry({ sender: sender('老王'), decision: 'allow_once' })} />);
  const line = screen.getByText('经插件 chat-bot 由 QQ 用户 老王 批准（仅此一次）');
  expect(line.classList.contains('agent-tool-brief')).toBe(true);
});

test('a call decided in Aura says nothing more', () => {
  const view = render(<ToolActivity entry={entry()} />);
  expect(view.container.textContent).not.toContain('经插件');
});
