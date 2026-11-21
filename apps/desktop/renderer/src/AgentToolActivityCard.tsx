import { userMessage } from './agent-session/user-message';
import type { AgentToolActivityEntry, PermissionMode } from '@aura/contracts/generated';
import { useCallback, useEffect, useRef, useState } from 'react';
import { ShellExecutionDetails } from './ShellExecutionDetails';
import { toolCallStateLabel } from './tool-call-state-label';
import { terminalReasonLabel } from './agent-session/tool-call-state';

/** Current V2 Tool history; the old P07 activity projection remains historical. */
export function AgentToolActivityCard({
  sessionId,
  currentMode,
  runtimeId,
}: {
  sessionId: string;
  currentMode?: PermissionMode | undefined;
  runtimeId?: string | undefined;
}) {
  return (
    <SessionAgentToolActivity
      key={`${sessionId}:${runtimeId}`}
      sessionId={sessionId}
      currentMode={currentMode}
    />
  );
}

function SessionAgentToolActivity({
  sessionId,
  currentMode,
}: {
  sessionId: string;
  currentMode: PermissionMode | undefined;
}) {
  const [calls, setCalls] = useState<AgentToolActivityEntry[]>([]);
  const [unreadable, setUnreadable] = useState<ReadonlySet<string>>(() => new Set());
  const [before, setBefore] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const alive = useRef(true);
  const pending = useRef(false);
  const [expanded, setExpanded] = useState(() => new Set<string>());
  const read = useCallback(
    async (cursor: string | null) => {
      if (pending.current) return;
      pending.current = true;
      setBusy(true);
      try {
        const result = await window.aura.v1.readAgentToolActivity({
          session_id: sessionId,
          before_call_id: cursor,
          limit: 20,
        });
        if (!alive.current) return;
        if (!result.ok) {
          setError(userMessage(result.error, '工具活动读取失败，请重试。'));
          return;
        }
        if (result.value.session_id !== sessionId) return;
        setCalls((previous) => {
          const incoming = new Map(result.value.calls.map((v) => [v.summary.tool_call_id, v]));
          const unavailable = new Set(result.value.unreadable_call_ids);
          const known = previous.filter((v) => !unavailable.has(v.summary.tool_call_id));
          if (cursor)
            return [
              ...known,
              ...result.value.calls.filter(
                (v) => !known.some((p) => p.summary.tool_call_id === v.summary.tool_call_id),
              ),
            ];
          return [
            ...result.value.calls,
            ...known.filter((v) => !incoming.has(v.summary.tool_call_id)),
          ];
        });
        setBefore((previous) =>
          cursor !== null || previous === null
            ? (result.value.next_before_call_id ?? null)
            : previous,
        );
        setUnreadable((previous) => {
          const unavailable = result.value.unreadable_call_ids ?? [];
          if (!previous.size && !unavailable.length) return previous;
          const next = new Set(previous);
          for (const call of result.value.calls) next.delete(call.summary.tool_call_id);
          for (const id of unavailable) next.add(id);
          return next;
        });
        setError(null);
      } catch {
        if (alive.current) setError('暂时无法读取工具活动。');
      } finally {
        pending.current = false;
        if (alive.current) setBusy(false);
      }
    },
    [sessionId],
  );
  useEffect(() => {
    alive.current = true;
    const start = window.setTimeout(() => void read(null), 0);
    const timer = window.setInterval(() => void read(null), 5000);
    return () => {
      alive.current = false;
      window.clearTimeout(start);
      window.clearInterval(timer);
    };
  }, [read]);
  if (!calls.length && !unreadable.size && !error) return null;
  return (
    <section className="session-tool-activity-card agent-tool-activity" aria-label="当前工具活动">
      <header>
        <div>
          <strong>工具执行记录</strong>
          <p>调用结果供模型判断；任务完成不会停止后台程序。</p>
        </div>
      </header>
      <ol>
        {calls.map(({ summary: call, shell_query_available, permission_mode }) => (
          <li key={call.tool_call_id}>
            <div className="agent-tool-call-summary">
              <strong>{call.display_name}</strong>
              <small>{terminalReasonLabel(call) ?? toolCallStateLabel(call)}</small>
              {permission_mode ? (
                <small>
                  执行权限：
                  {
                    {
                      read_only: '只读',
                      workspace_read_write: '工作区读写',
                      full_access: '完全访问',
                    }[permission_mode]
                  }
                  {call.status === 'running' && currentMode && currentMode !== permission_mode
                    ? '（按启动时权限继续）'
                    : ''}
                </small>
              ) : null}
              {shell_query_available ? (
                <button
                  type="button"
                  aria-expanded={expanded.has(call.tool_call_id)}
                  onClick={() =>
                    setExpanded((previous) => {
                      const next = new Set(previous);
                      if (next.has(call.tool_call_id)) next.delete(call.tool_call_id);
                      else next.add(call.tool_call_id);
                      return next;
                    })
                  }
                >
                  {expanded.has(call.tool_call_id) ? '收起记录' : '脚本、输出与进程状态'}
                </button>
              ) : null}
            </div>
            {expanded.has(call.tool_call_id) ? (
              <ShellExecutionDetails sessionId={sessionId} callId={call.tool_call_id} />
            ) : null}
          </li>
        ))}
      </ol>
      {unreadable.size > 0 && <p role="status">{unreadable.size} 条工具记录暂时无法读取。</p>}
      {before ? (
        <button type="button" disabled={busy} onClick={() => void read(before)}>
          加载更早调用
        </button>
      ) : null}
      {error ? <p role="alert">{error}</p> : null}
    </section>
  );
}
