import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { useState } from 'react';
import { createRoot } from 'react-dom/client';
import { ToolActivity } from '../../renderer/src/agent-session/ToolActivity';
import { ToolCallChip } from '../../renderer/src/agent-session/ToolCallChip';
import {
  groupConsecutiveToolCalls,
  isGroupedRun,
} from '../../renderer/src/agent-session/tool-call-group';
import { ToolCallRun } from '../../renderer/src/agent-session/tool-run-row';
import '../../renderer/src/styles/index.css';
import '../../renderer/src/agent-session/agent-session.css';
import '../../renderer/src/agent-session/turn-process.css';
import './preview.css';

type Summary = AgentToolActivityEntry['summary'];
const longTarget = `src/${'long-directory-'.repeat(24)}missing`;
function entry(id: string, patch: Partial<Summary>, target = longTarget): AgentToolActivityEntry {
  return {
    summary: {
      tool_call_id: id,
      tool_batch_id: 'fixture-batch',
      ordinal: 1,
      tool_id: 'fixture-fs-list',
      tool_version: { definition_revision: 1, semantic_version: null },
      display_name: 'fs.list',
      status: 'finalized',
      outcome: { type: 'operation_rejected' },
      side_effect_state: 'not_started',
      started_at: null,
      completed_at: null,
      can_cancel: false,
      ...patch,
    },
    model_step_id: 'fixture-step',
    display: {
      title: '列出目录',
      target,
      summary: '诊断示例；合成数据，不访问真实文件。',
      details: [
        '仅验证真实组件的显示和交互；不连接 Core，也不发送模型请求。',
        `诊断字段：/_aura/${'long-field-'.repeat(35)}`,
        '_aura 只能含 purpose；将业务参数放在同级 arguments。',
      ],
      truncated: false,
      internal: false,
    },
    shell_query_available: false,
    permission_mode: 'read_only',
    permission_revision: 1,
  } as AgentToolActivityEntry;
}

function Preview() {
  const [narrow, setNarrow] = useState(false);
  const [finished, setFinished] = useState(false);
  const entries = [
    entry('missing-a', { error_class: 'not_found' }, 'removed-a'),
    entry('missing-b', { error_class: 'not_found' }, 'removed-b'),
    entry('denied', { error_class: 'permission' }),
    entry('invalid', {
      error_class: 'invalid_request',
      outcome: { type: 'not_executed', reason: 'schema_validation_failed' },
    }),
    entry('unknown', { error_class: 'permission', side_effect_state: 'unknown' }),
    ...['running-a', 'running-b'].map((id) =>
      entry(
        id,
        {
          status: finished ? 'finalized' : 'running',
          outcome: finished ? { type: 'succeeded' } : null,
          side_effect_state: finished ? 'fully_applied' : null,
        },
        id,
      ),
    ),
    entry('queued', { status: 'queued', outcome: null, side_effect_state: null }),
  ];
  const calls = new Map(entries.map((value) => [value.summary.tool_call_id, value]));
  const groups = groupConsecutiveToolCalls(
    entries.map((value) => ({ callId: value.summary.tool_call_id, entry: value })),
  );
  return (
    <main className="preview">
      <h1>工具状态与诊断</h1>
      <p>独立 UI 夹具 · 使用生产组件与样式 · 无真实 Core / 模型调用</p>
      <nav aria-label="验证控制">
        <button type="button" onClick={() => setNarrow((value) => !value)}>
          {narrow ? '切换宽布局' : '切换窄布局（360px）'}
        </button>
        <button type="button" onClick={() => setFinished((value) => !value)}>
          {finished ? '恢复运行中' : '完成运行中调用'}
        </button>
      </nav>
      <section
        className="agent-process preview-panel"
        aria-label="工具调用"
        style={{ width: narrow ? 360 : 840 }}
      >
        {groups.map((group) =>
          isGroupedRun(group) ? (
            <ToolCallRun key={group.key} group={group} calls={calls} />
          ) : (
            <ToolActivity key={group.key} entry={calls.get(group.callIds[0]!)} />
          ),
        )}
        <h2>同一未知结果的详情 Chip</h2>
        <ToolCallChip call={entries[4]!.summary} callId="unknown" />
      </section>
    </main>
  );
}

createRoot(document.getElementById('root')!).render(<Preview />);
