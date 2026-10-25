import type { ToolCallActivitySummaryView } from '@aura/contracts/generated';
import { ExpandIcon } from '../tool-icon';
import { userMessage } from './user-message';
import { ToolCallChip } from './ToolCallChip';
import { useDisclosureOpen } from './disclosure-open';
import { useRef, useState } from 'react';
import { ScrollBox } from '../ui/ScrollBox';
export function ToolResult({
  sessionId,
  callId,
  call,
}: {
  sessionId: string;
  callId: string;
  call?: ToolCallActivitySummaryView | undefined;
}) {
  const [content, setContent] = useState<string>();
  const [offset, setOffset] = useState<number | null>(0);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const { onToggle, open } = useDisclosureOpen();
  const reading = useRef(false);
  const load = async () => {
    if (reading.current || offset === null) return;
    reading.current = true;
    setBusy(true);
    try {
      const result = await window.aura.v1.readAgentResult({
        session_id: sessionId,
        tool_call_id: callId,
        offset,
        limit: 16_384,
      });
      if (!result.ok) {
        setError(result.error.message);
        return;
      }
      setContent((old) => (old ?? '') + result.value.text);
      setOffset(result.value.next_offset ?? null);
      setError(undefined);
    } catch {
      setError('无法读取结果。');
    } finally {
      reading.current = false;
      setBusy(false);
    }
  };
  return (
    <details
      className="agent-result"
      onToggle={(event) => {
        onToggle(event);
        if (event.currentTarget.open && content === undefined) void load();
      }}
    >
      <summary>
        <span className="agent-tool-chevron" aria-hidden="true">
          <ExpandIcon open={open} />
        </span>
        <ToolCallChip call={call} callId={callId} />
      </summary>
      <p className="agent-result-identity">调用 ID：{callId}</p>
      {content === undefined ? (
        <p role="status">{busy ? '读取中…' : '结果尚未就绪'}</p>
      ) : (
        <ScrollBox label="工具结果">
          <pre>{content}</pre>
        </ScrollBox>
      )}
      {content !== undefined && offset !== null ? (
        <button type="button" disabled={busy} onClick={() => void load()}>
          继续读取完整结果
        </button>
      ) : null}
      {error && content === undefined ? (
        <button type="button" disabled={busy} onClick={() => void load()}>
          重新读取结果
        </button>
      ) : null}
      {error ? <p role="alert">{userMessage(error)}</p> : null}
    </details>
  );
}
