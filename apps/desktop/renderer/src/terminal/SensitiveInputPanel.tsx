import type { TerminalSensitiveRequestV1 } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import { toolTabs } from '../right-workspace/tool-tabs';
import { terminalApi } from './store';
import './sensitive-input.css';
import { SENSITIVE_UNCONFIRMED, sensitiveFailure } from './failure-words';

/** Core's refusal when the terminal no longer shows the prompt the request was opened at. */
const PROMPT_GONE = 'terminal.sensitive.prompt_gone';

export function SensitiveInputPanel({ sessionId }: { sessionId: string }) {
  const [requests, setRequests] = useState<TerminalSensitiveRequestV1[]>([]);
  // Answered after their prompt had gone: nothing was typed, and Core ended them. Each stays here
  // until the user closes it.
  const [gone, setGone] = useState<TerminalSensitiveRequestV1[]>([]);
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
  const ended = (id: string) => gone.some((request) => request.tool_call_id === id);
  return (
    <>
      {requests
        .filter((request) => !ended(request.tool_call_id))
        .map((request) => (
          <SensitiveInputForm
            key={request.tool_call_id}
            request={request}
            onPromptGone={(answered) =>
              setGone((list) =>
                list.some((request) => request.tool_call_id === answered.tool_call_id)
                  ? list
                  : [...list, answered],
              )
            }
          />
        ))}
      {gone.map((request) => (
        <SensitivePromptGone
          key={request.tool_call_id}
          request={request}
          onClose={() =>
            setGone((list) => list.filter((item) => item.tool_call_id !== request.tool_call_id))
          }
        />
      ))}
    </>
  );
}
export function SensitiveInputForm({
  request,
  onPromptGone,
}: {
  request: TerminalSensitiveRequestV1;
  /** Called even after the form is gone: Core may end the request before its answer arrives. */
  onPromptGone?: (request: TerminalSensitiveRequestV1) => void;
}) {
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
      if (!result.ok && !cancel && result.error.code === PROMPT_GONE) onPromptGone?.(request);
      else if (alive.current && !result.ok) setError(sensitiveFailure(result.error));
      else if (alive.current && result.ok && result.value.status === 'failed')
        setError('终端未确认接收输入，请查看终端当前状态。');
    } catch {
      if (alive.current) setError(SENSITIVE_UNCONFIRMED);
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
/** The terminal had moved on from the prompt when the user answered, so nothing was typed. */
function SensitivePromptGone({
  request,
  onClose,
}: {
  request: TerminalSensitiveRequestV1;
  onClose: () => void;
}) {
  return (
    <section className="terminal-sensitive-input" aria-label="终端安全输入">
      <strong>终端 · 安全输入</strong>
      <p>{request.prompt}</p>
      <p role="alert">终端已不在等这次输入，没有输入。请查看终端，需要时让模型再请求一次。</p>
      <div>
        <button
          type="button"
          onClick={() => toolTabs.revealTerminal(request.session_id, request.terminal_id)}
        >
          查看终端
        </button>
        <button type="button" onClick={onClose}>
          关闭
        </button>
      </div>
    </section>
  );
}
