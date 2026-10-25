import { userMessage } from '../agent-session/user-message';
import { useEffect, useRef, useState } from 'react';
import { addTerminal, terminalApi, type TerminalRecord } from './store';
import { mountTerminal, terminalEngine } from './xterm-runtime';
import { ProjectionCompatibilityNotice } from '../ProjectionCompatibilityNotice';

/** One terminal's screen with its status line and, for the model's terminal, takeover. */
export function TerminalScreen({ record, active }: { record: TerminalRecord; active: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (host.current) return mountTerminal(record, host.current);
    return undefined;
  }, [record]);
  useEffect(() => {
    if (active) terminalEngine(record).terminal.focus();
  }, [active, record]);
  useEffect(() => {
    terminalEngine(record).terminal.options.disableStdin =
      record.view.controller === 'agent' || !!record.view.end;
  }, [record, record.view.controller, record.view.end]);
  const [controlError, setControlError] = useState('');
  // One takeover or hand-back at a time: a second click would ask for the opposite controller.
  const [transferring, setTransferring] = useState(false);
  const transfer = async () => {
    if (transferring) return;
    setTransferring(true);
    setControlError('');
    try {
      const result = await terminalApi()?.control({
        session_id: record.view.session_id,
        terminal_id: record.view.terminal_id,
        controller: record.view.controller === 'agent' ? 'user' : 'agent',
      });
      if (result?.ok) await addTerminal(result.value);
      else setControlError(userMessage(result?.error, '接管或交还没有完成，请重试。'));
    } finally {
      setTransferring(false);
    }
  };
  return (
    <div className="terminal-view" hidden={!active}>
      <div className="terminal-status" role="status">
        控制：{record.view.controller === 'agent' ? 'Agent' : '用户'} ·{' '}
        {record.view.end
          ? `已结束（${record.view.end.reason === 'core_stopped' ? 'Core 已停止' : record.view.end.reason === 'exited' ? `退出码 ${record.view.end.exit_code ?? '未知'}` : '已关闭'}）`
          : '运行中'}
        {record.view.owner === 'agent' && !record.view.end ? (
          <button
            type="button"
            aria-busy={transferring || undefined}
            disabled={transferring}
            onClick={() => {
              void transfer();
            }}
          >
            {record.view.controller === 'agent' ? '接管' : '交还'}
          </button>
        ) : null}
      </div>
      {controlError ? (
        <p role="alert" className="terminal-error">
          {controlError}
        </p>
      ) : null}
      {record.error ? (
        <p className="terminal-error" role="alert">
          {record.error}
        </p>
      ) : null}
      <ProjectionCompatibilityNotice
        surface="terminal"
        sessionId={record.view.session_id}
        terminalId={record.view.terminal_id}
      />
      <div className="terminal-host" aria-label="终端输入和输出" ref={host} />
    </div>
  );
}
