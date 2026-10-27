import type { AgentTurnView } from '@aura/contracts/generated';
import { useEffect, useState } from 'react';

/** Below this many quiet seconds the turn is still visibly writing; nothing needs saying. */
const QUIET_SECONDS = 2;

/** A size as people read it. */
function readableSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} 字节`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const fileName = (path: string) => path.split(/[\\/]/u).filter(Boolean).at(-1) ?? path;

/**
 * When each step's
 * latest output was first seen, kept by the window rather than by one view of it, so switching
 * conversations and back goes on counting. The oldest are forgotten first.
 */
const firstSeen = new Map<string, number>();
const REMEMBERED = 200;

function remember(key: string, since: number): void {
  if (firstSeen.has(key)) return;
  firstSeen.set(key, since);
  if (firstSeen.size > REMEMBERED) firstSeen.delete(firstSeen.keys().next().value!);
}

/** Whole seconds since `key` last changed (or since `since`, when known), while there is a key. */
function useQuietSeconds(key: string | undefined, since?: number): number {
  const known = since !== undefined && Number.isFinite(since) ? since : undefined;
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (key === undefined) return;
    remember(key, known ?? Date.now());
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [key, known]);
  if (key === undefined) return 0;
  // Before its first sight is recorded, a key with no start of its own has been quiet for 0 s.
  const start = firstSeen.get(key) ?? known ?? now;
  return Math.max(0, Math.floor((now - start) / 1000));
}

/**
 * "正文仍在生成，会持续补齐。" stayed for minutes with nothing moving — the model
 * was writing a whole file into a tool call, which nothing showed — and "让人感觉就像卡住了一样".
 * At the end of a running turn: the tool call being written, its file and how much of it has
 * arrived, growing as it streams; nothing while words are arriving; and after a pause, that the
 * model is thinking and for how long.
 */
export function StepActivity({ item }: { item: AgentTurnView }) {
  const latest = item.assistant_messages.at(-1);
  const live = item.turn.state.state === 'running' && latest?.partial ? latest : undefined;
  const inputs = live?.tool_input ?? [];
  const writing = inputs.findLast((input) => !input.complete);
  const arrived =
    (live?.text_total_bytes ?? 0) +
    (live?.reasoning_total_bytes ?? 0) +
    inputs.reduce((sum, input) => sum + input.received_bytes, 0);
  // A step whose context is being compacted waits on that, counted from when the part running now
  // began; any other step that has written nothing yet has been quiet since it started.
  const compacting = live?.context_compacting;
  const quiet = useQuietSeconds(
    live
      ? compacting
        ? `${live.model_step_id}:compacting:${compacting.since}`
        : `${live.model_step_id}:${arrived}`
      : undefined,
    compacting
      ? Date.parse(compacting.since)
      : live && arrived === 0
        ? Date.parse(live.at)
        : undefined,
  );
  if (!live) return null;
  if (compacting)
    return (
      <p className="agent-step-activity" role="status">
        正在压缩上下文 · {quiet} 秒
        {compacting.finished_parts > 0 ? ` · 已完成 ${compacting.finished_parts} 段` : ''}
      </p>
    );
  if (writing)
    return (
      <p className="agent-step-activity" role="status">
        正在编写「{writing.title}」{writing.target ? fileName(writing.target) : ''} · 已生成{' '}
        {readableSize(writing.received_bytes)}
      </p>
    );
  if (quiet < QUIET_SECONDS) return null;
  return (
    <p className="agent-step-activity" role="status">
      正在思考下一步 · {quiet} 秒
    </p>
  );
}
