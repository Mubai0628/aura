import { useEffect, useRef, useState } from 'react';

/** How much of a thought's end is read for its newest line. */
const TAIL_BYTES = 768;
/** A thought being written is read again at most this often. */
const INTERVAL_MS = 400;
/** A longer line keeps its newest words. */
const LINE_CHARACTERS = 90;

const encoded = (text: string) => new TextEncoder().encode(text).length;

/** The newest non-empty line of `text`; a long one keeps its end. */
export function newestLine(text: string): string {
  const line =
    text
      .split('\n')
      .map((value) => value.trim())
      .findLast((value) => value.length > 0) ?? '';
  const characters = [...line];
  return characters.length > LINE_CHARACTERS
    ? `…${characters.slice(-LINE_CHARACTERS).join('')}`
    : line;
}

/**
 * A closed
 * thought's newest line: while it is written, as it grows, read from its end at most every 0.4 s;
 * once written, its last. A thought its preview holds whole needs no read.
 */
export function useReasoningTail({
  sessionId,
  modelStepId,
  preview,
  totalBytes,
  partial,
  open,
}: {
  sessionId: string;
  modelStepId: string;
  preview: string;
  totalBytes: number;
  partial: boolean;
  open: boolean;
}): string {
  const whole = encoded(preview) >= totalBytes;
  const [tail, setTail] = useState<string>();
  const last = useRef(0);
  useEffect(() => {
    if (open || whole) return;
    let alive = true;
    const wait = partial ? Math.max(0, last.current + INTERVAL_MS - Date.now()) : 0;
    const timer = window.setTimeout(async () => {
      last.current = Date.now();
      try {
        const result = await window.aura.v1.readAgentMessageText({
          session_id: sessionId,
          source: { kind: 'reasoning', model_step_id: modelStepId },
          offset: Math.max(0, totalBytes - TAIL_BYTES),
          limit: TAIL_BYTES,
        });
        if (alive && result.ok) setTail(newestLine(result.value.text));
      } catch {
        // The line stays what the preview says; the next growth reads again.
      }
    }, wait);
    return () => {
      alive = false;
      window.clearTimeout(timer);
    };
  }, [modelStepId, open, partial, sessionId, totalBytes, whole]);
  return whole || !tail ? newestLine(preview) : tail;
}
