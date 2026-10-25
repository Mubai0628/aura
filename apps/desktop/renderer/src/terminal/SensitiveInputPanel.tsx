import { userMessage } from '../agent-session/user-message';
import type { TerminalSensitiveRequestV1 } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { terminalApi } from './store';
import './sensitive-input.css';
export function SensitiveInputPanel({ sessionId }: { sessionId: string }) {
  const [requests, setRequests] = useState<TerminalSensitiveRequestV1[]>([]);
  useEffect(() => {
    const api = terminalApi();
    if (!api) return;
    let alive = true;
    let reading = false;
    const apply = (pending: { session_id: string; requests: TerminalSensitiveRequestV1[] }) => {
      if (alive && pending.session_id === sessionId) setRequests(pending.requests);
    };
    const unsubscribe = api.onSensitive(apply);
    const refresh = async () => {
      if (reading) return;
      reading = true;
      try {
        const result = await api.pendingSensitive({ session_id: sessionId });
        if (result.ok) apply(result.value);
        else if (alive) setRequests([]);
      } catch {
        if (alive) setRequests([]);
      } finally {
        reading = false;
      }
    };
    void refresh();
    const timer = window.setInterval(() => void refresh(), 2000);
    return () => {
      alive = false;
      unsubscribe();
      window.clearInterval(timer);
    };
  }, [sessionId]);
  return requests.map((request) => (
    <SensitiveInputForm key={request.tool_call_id} request={request} />
  ));
}
export function SensitiveInputForm({ request }: { request: TerminalSensitiveRequestV1 }) {
  const input = useRef<HTMLInputElement>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const alive = useRef(true);
  useEffect(() => {
    const node = input.current;
    alive.current = true;
    node?.focus();
    return () => {
      alive.current = false;
      if (node) node.value = '';
    };
  }, []);
  const reply = { session_id: request.session_id, tool_call_id: request.tool_call_id };
  const send = async (cancel: boolean) => {
    const api = terminalApi();
    if (!api || busy) return;
    const value = input.current;
    if (!cancel && !value?.value) {
      setError('请输入内容，或取消本次请求。');
      return;
    }
    let bytes: Uint8Array | undefined;
    setBusy(true);
    setError('');
    try {
      if (!cancel) bytes = new TextEncoder().encode(value!.value);
      if (value) value.value = '';
      const result = cancel
        ? await api.cancelSensitive(reply)
        : await api.submitSensitive(reply, bytes!);
      if (alive.current && !result.ok)
        setError(userMessage(result.error, '安全输入没有完成，请重试。'));
      else if (alive.current && result.ok && result.value.status === 'failed')
        setError('终端未确认接收输入，请查看终端当前状态。');
    } catch {
      if (alive.current) setError('安全输入未确认提交，请查看当前请求；不要自动重发。');
    } finally {
      bytes?.fill(0);
      if (value) value.value = '';
      if (alive.current) setBusy(false);
    }
  };
  return (
    <form
      className="terminal-sensitive-input"
      aria-label="终端安全输入"
      onSubmit={(event) => {
        event.preventDefault();
        void send(false);
      }}
    >
      <strong>终端 · 安全输入</strong>
      <p>{request.prompt}</p>
      <label>
        安全输入
        <input ref={input} type="password" autoComplete="off" maxLength={2560} disabled={busy} />
      </label>
      <p className="terminal-sensitive-note">
        这段输入不会显示给模型，也不会被记录；如果程序会回显输入，终端里仍可能看到。
      </p>
      <div>
        <button type="submit" disabled={busy}>
          {busy ? '提交中…' : '提交'}
        </button>
        <button type="button" disabled={busy} onClick={() => void send(true)}>
          取消
        </button>
      </div>
      {error ? <p role="alert">{error}</p> : null}
    </form>
  );
}
