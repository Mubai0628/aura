import { userMessage } from './agent-session/user-message';
import type { ShellExecutionView } from '@aura/contracts/generated';
import { useCallback, useEffect, useRef, useState } from 'react';

const labels: Record<ShellExecutionView['state'], string> = {
  waiting: '未完成 · 等待执行',
  running: '未完成 · 直接执行中',
  completed: '调用已完成',
  failed: '工具调用失败',
  interrupted: '已中断 · 未正常完成',
  indeterminate: '直接执行状态暂不确定',
};
const backgroundLabels = {
  running: '仍在运行',
  exited: '已退出',
  interrupted: '已停止',
  unknown: '状态暂不确定',
};

export function ShellExecutionDetails({
  sessionId,
  callId,
}: {
  sessionId: string;
  callId: string;
}) {
  return (
    <ShellExecutionRecord key={`${sessionId}:${callId}`} sessionId={sessionId} callId={callId} />
  );
}

function ShellExecutionRecord({ sessionId, callId }: { sessionId: string; callId: string }) {
  const [view, setView] = useState<ShellExecutionView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const current = useRef<ShellExecutionView | null>(null);
  const pending = useRef(false);
  const alive = useRef(true);
  const query = useCallback(async () => {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      const previous = current.current;
      const result = await window.aura.v1.readShellExecution({
        session_id: sessionId,
        input: {
          tool_call_id: callId,
          stdout_offset: previous?.stdout.length ?? 0,
          stderr_offset: previous?.stderr.length ?? 0,
          max_bytes: 16384,
        },
      });
      if (!alive.current) return;
      if (!result.ok) {
        setError(userMessage(result.error, '命令输出读取失败，请重试。'));
        return;
      }
      const next = {
        ...result.value,
        stdout: [...(previous?.stdout ?? []), ...result.value.stdout],
        stderr: [...(previous?.stderr ?? []), ...result.value.stderr],
      };
      current.current = next;
      setView(next);
      setError(null);
    } catch {
      if (alive.current) setError('暂时无法读取执行记录，请稍后刷新。');
    } finally {
      pending.current = false;
      if (alive.current) setBusy(false);
    }
  }, [sessionId, callId]);
  useEffect(() => {
    alive.current = true;
    void query();
    const timer = window.setInterval(() => {
      const value = current.current;
      if (
        value?.state === 'running' ||
        value?.state === 'waiting' ||
        value?.background === 'running'
      )
        void query();
    }, 3000);
    return () => {
      alive.current = false;
      window.clearInterval(timer);
    };
  }, [query]);
  async function stop() {
    if (pending.current) return;
    pending.current = true;
    setBusy(true);
    try {
      const result = await window.aura.v1.stopShellExecution({
        session_id: sessionId,
        input: { tool_call_id: callId },
      });
      if (alive.current && !result.ok)
        setError(userMessage(result.error, '停止没有完成，请刷新查看实际状态。'));
    } catch {
      if (alive.current) setError('停止请求未取得结果；可刷新查看实际状态。');
    } finally {
      pending.current = false;
      if (alive.current) {
        setBusy(false);
        void query();
      }
    }
  }
  const decode = (bytes: number[]) => new TextDecoder().decode(new Uint8Array(bytes));
  return (
    <section className="shell-execution-details" aria-label="Shell 执行记录">
      {view ? (
        <>
          <p>
            {labels[view.state]}
            {view.exit_code !== null && view.exit_code !== undefined
              ? ` · 退出码 ${view.exit_code}`
              : ''}
          </p>
          {view.background ? (
            <p>
              托管后台进程：{backgroundLabels[view.background]}。任务结束后可继续运行，退出 Aura
              时停止。
            </p>
          ) : null}
          {view.diagnostic ? <p>{view.diagnostic}</p> : null}
          <dl>
            <dt>工作目录</dt>
            <dd>{view.command.working_directory}</dd>
            <dt>执行环境</dt>
            <dd>
              {view.command.shell} · {view.environment_version}
            </dd>
            <dt>开始时间</dt>
            <dd>{view.started_at ?? '尚未取得启动记录'}</dd>
            <dt>直接执行结束</dt>
            <dd>{view.finished_at ?? '尚未取得结束记录'}</dd>
          </dl>
          <details>
            <summary>原始命令与输入</summary>
            <pre>{JSON.stringify(view.original_input, null, 2)}</pre>
          </details>
          {view.submitted_script ? (
            <details>
              <summary>实际提交的脚本快照</summary>
              <pre>{view.submitted_script}</pre>
            </details>
          ) : null}
          <h4>标准输出</h4>
          <pre>{decode(view.stdout) || '（无输出）'}</pre>
          <h4>标准错误</h4>
          <pre>{decode(view.stderr) || '（无输出）'}</pre>
          {view.output_truncated ? <p>输出达到保留上限，超出部分未保存。</p> : null}
          {view.stdout_next || view.stderr_next ? (
            <button type="button" disabled={busy} onClick={() => void query()}>
              读取更多已保存输出
            </button>
          ) : null}
          {view.state === 'running' || view.background === 'running' ? (
            <button type="button" disabled={busy} onClick={() => void stop()}>
              停止本次托管进程
            </button>
          ) : null}
        </>
      ) : (
        <p>正在读取执行记录…</p>
      )}
      <button type="button" disabled={busy} onClick={() => void query()}>
        {busy ? '读取中…' : '刷新状态'}
      </button>
      {error ? <p role="alert">{error}</p> : null}
    </section>
  );
}
