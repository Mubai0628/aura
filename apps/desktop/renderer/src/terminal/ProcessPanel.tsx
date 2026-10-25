import { userMessage } from '../agent-session/user-message';
import type { ManagedProcessViewV1 } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { ShellExecutionDetails } from '../ShellExecutionDetails';
import { terminalApi } from './store';
import './process-panel.css';
export function ProcessPanel({
  sessionId,
  onViewTerminal,
}: {
  sessionId?: string | undefined;
  onViewTerminal: (id: string) => void;
}) {
  return sessionId ? (
    <Processes key={sessionId} sessionId={sessionId} onViewTerminal={onViewTerminal} />
  ) : (
    <p className="process-empty">请先选择会话。</p>
  );
}
function Processes({
  sessionId,
  onViewTerminal,
}: {
  sessionId: string;
  onViewTerminal: (id: string) => void;
}) {
  const [processes, setProcesses] = useState<ManagedProcessViewV1[]>([]);
  const [error, setError] = useState('');
  const [stopError, setStopError] = useState('');
  const [selected, setSelected] = useState('');
  const [busy, setBusy] = useState('');
  const refresh = useRef<() => Promise<void>>(async () => {});
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    let pending = false;
    refresh.current = async () => {
      if (pending) return;
      pending = true;
      try {
        const result = await terminalApi()?.listProcesses({ session_id: sessionId });
        if (alive.current && result) {
          if (result.ok) {
            setProcesses(result.value.processes);
            setError('');
          } else setError(userMessage(result.error, '进程列表读取失败，请重试。'));
        }
      } catch {
        if (alive.current) setError('暂时无法读取进程，请刷新重试。');
      } finally {
        pending = false;
      }
    };
    void refresh.current();
    const timer = window.setInterval(() => void refresh.current(), 1000);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
    };
  }, [sessionId]);
  const stop = async (toolCallId: string) => {
    setStopError('');
    setBusy(toolCallId);
    try {
      const result = await terminalApi()?.stopProcess({
        session_id: sessionId,
        tool_call_id: toolCallId,
      });
      if (alive.current && result && (!result.ok || !result.value.stopped))
        setStopError(
          result.ok
            ? '停止尚未确认，请刷新查看实际状态。'
            : userMessage(result.error, '停止没有完成，请刷新查看实际状态。'),
        );
    } catch {
      if (alive.current) setStopError('停止尚未确认，请刷新查看实际状态。');
    } finally {
      if (alive.current) {
        setBusy('');
        void refresh.current();
      }
    }
  };
  return (
    <section className="process-panel" aria-label="会话后台进程">
      <div className="process-toolbar">
        <span>模型的后台进程</span>
        <button type="button" onClick={() => void refresh.current()}>
          刷新
        </button>
      </div>
      {error ? <p role="alert">{error}</p> : null}
      {stopError ? <p role="alert">{stopError}</p> : null}
      {processes.length ? (
        <ul>
          {processes.map((process) => (
            <li key={process.tool_call_id}>
              <strong>{process.kind === 'terminal' ? 'Agent 终端' : 'Shell 后台进程'}</strong>
              <pre>{process.command}</pre>
              <p>开始于 {new Date(process.started_at).toLocaleString()}</p>
              <p className="process-call">工具调用 {process.tool_call_id}</p>
              <div>
                <button
                  type="button"
                  onClick={() =>
                    process.terminal_id
                      ? onViewTerminal(process.terminal_id)
                      : setSelected(process.tool_call_id)
                  }
                >
                  {process.terminal_id ? '查看终端' : '查看输出'}
                </button>
                <button
                  type="button"
                  disabled={!!busy}
                  onClick={() => void stop(process.tool_call_id)}
                >
                  {busy === process.tool_call_id ? '停止中…' : '停止'}
                </button>
              </div>
            </li>
          ))}
        </ul>
      ) : (
        <p className="process-empty">本会话没有仍在运行的后台进程。</p>
      )}
      {selected ? (
        <>
          <button type="button" onClick={() => setSelected('')}>
            关闭输出记录
          </button>
          <ShellExecutionDetails sessionId={sessionId} callId={selected} />
        </>
      ) : null}
    </section>
  );
}
