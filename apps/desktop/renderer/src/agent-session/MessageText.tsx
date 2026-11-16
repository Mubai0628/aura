import type { AgentMessageTextSource } from '@aura/contracts/generated';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { MarkdownBlocks } from '../markdown-body';
import { pageCacheKey, readPageCache, writePageCache } from './message-text-cache';
import { messageTextQueue } from './message-text-queue';
import { MentionContext, mentionOf } from '../files/mention-context';
import { failedAction } from './failure-text';

/** Roles that carry a real answer, a real person's words or the model's own process notes
 *  are Markdown. Model reasoning and program-made tool briefs stay plain text. */
export type MessageTextRole = 'user' | 'final' | 'reasoning' | 'process' | 'brief';

const PLAIN_ROLES: readonly MessageTextRole[] = ['reasoning', 'brief'];

/** How many bytes one automatic page reads: small enough to paint quickly,
 *  large enough that a long body arrives in a few passes. */
export const MESSAGE_TEXT_PAGE_BYTES = 32_768;
/** What a page of a body that was not read says; the part shown stays. */
const BODY_UNREAD = '这条消息后面的内容没有读到';
const BODY_NEXT = '已显示的部分不受影响；点“重新读取正文”再试。';

/** A viewport margin so a body just below the fold loads before it is read. */
const VIEWPORT_MARGIN = '600px';

const sourceId = (source: AgentMessageTextSource): string => {
  if (source.kind === 'user') return source.event_id;
  if (source.kind === 'completion') return source.turn_task_id;
  return source.model_step_id;
};

/** One queue id per immutable source, unique across sessions. */
const queueId = (sessionId: string, source: AgentMessageTextSource): string =>
  `${sessionId}:${source.kind}:${sourceId(source)}`;

/** Confirms a page belongs to the same immutable source. A page whose source
 *  changed is rejected instead of being appended to another body. */
const matchesSource = (page: AgentMessageTextSource, source: AgentMessageTextSource): boolean =>
  page.kind === source.kind && sourceId(page) === sourceId(source);

const pageBytes = (text: string): number => new TextEncoder().encode(text).length;

/** The caller has already keyed the cache by the authoritative sealed revision.
 *  The size and preview also have to agree; a prefix alone is never an identity. */
const cachedBody = (
  cached: string | undefined,
  preview: string,
  totalBytes: number,
): string | undefined => {
  if (cached === undefined) return undefined;
  if (pageBytes(cached) === totalBytes && cached.startsWith(preview)) return cached;
  return undefined;
};

/** Preview and subsequent pages all refer to the same immutable source, or to
 *  the same live body while the step is still running. */
export function MessageText({
  sessionId,
  source,
  preview,
  totalBytes,
  partial = false,
  bodyId = null,
  sealedRevision = null,
  role = 'brief',
  className,
}: {
  sessionId: string;
  source: AgentMessageTextSource;
  preview: string;
  totalBytes: number;
  /** The stored body is still the live stream. This is independent of whether
   *  the turn is still producing text. The turn itself says what it is doing
   *  (`StepActivity`); a body shows no fixed “still generating” line. */
  partial?: boolean;
  /** The live body the projection currently reports for this source. A page
   *  from another body is never appended to this one. */
  bodyId?: string | null;
  /** Core-recorded identity of the sealed content. Legacy sources without one
   *  remain readable, but cannot reuse a suffix across component remounts. */
  sealedRevision?: string | null | undefined;
  role?: MessageTextRole;
  className?: string;
}) {
  const { busy, element, error, more, retry, text } = useProgressiveBody({
    sessionId,
    source,
    preview,
    totalBytes,
    partial,
    bodyId,
    sealedRevision,
  });
  const plain = PLAIN_ROLES.includes(role);
  // Plain text keeps the existing `className` exactly where it always was: on
  // the text element itself. A Markdown body carries its own block container,
  // so it only forwards a host class that names real block styling.
  const body =
    className === undefined || className === 'agent-message-text' ? undefined : className;
  // The text on screen is a prefix until the loader reports the body complete, so the
  // renderer stays in its incremental mode until then. `partial` says the model is
  // still producing this body; `more` is the loader's own exhaustion fact — source
  // identity plus byte offsets, not a string length — and it stays true while a
  // sealed answer is still being paged. Parsing an incomplete document as final text
  // is what mangles an unterminated construct, so both facts keep streaming on.
  const streaming = partial || more;
  const mentionKey = `${source.kind}:${sourceId(source)}`;
  // eslint-disable-next-line react-hooks/exhaustive-deps -- the source's identity, not its object.
  const mention = useMemo(() => mentionOf(source), [mentionKey]);
  // A full path outside the workspace in this text opens, found in this message first.
  return (
    <MentionContext.Provider value={mention}>
      <div aria-busy={busy} ref={element}>
        {plain ? (
          <p className={className ?? 'agent-message-text'}>{text}</p>
        ) : body === undefined ? (
          <MarkdownBlocks programs={role === 'user'} streaming={streaming} text={text} />
        ) : (
          <MarkdownBlocks
            className={body}
            programs={role === 'user'}
            streaming={streaming}
            text={text}
          />
        )}
      </div>
      {error ? (
        <p role="alert">
          {error}
          <button type="button" onClick={retry}>
            重新读取正文
          </button>
        </p>
      ) : null}
    </MentionContext.Provider>
  );
}

/** Paging, offsets, viewport priority and failure handling for one body. The
 *  component above only decides how the text is rendered.
 *
 *  A body is read from two kinds of source. A sealed body never changes, so a
 *  page is an exact slice and the announced size is a promise. A running step's
 *  visible body grows while it is read and can be replaced by a retry, so each
 *  page carries the identity of the body it was cut from and the byte it really
 *  starts at: a page from another body is discarded and the body is read again
 *  from its first byte instead of being spliced into text it does not belong to.
 */
function useProgressiveBody({
  sessionId,
  source,
  preview,
  totalBytes,
  partial,
  bodyId,
  sealedRevision,
}: {
  sessionId: string;
  source: AgentMessageTextSource;
  preview: string;
  totalBytes: number;
  partial: boolean;
  bodyId: string | null;
  sealedRevision: string | null;
}) {
  const id = queueId(sessionId, source);
  // A sealed size is part of the identity. A live size can grow between reads.
  const projectionIdentity = [
    id,
    partial,
    bodyId,
    sealedRevision,
    preview,
    partial ? null : totalBytes,
  ];
  const projectionId = JSON.stringify(projectionIdentity);
  const sealedKey =
    !partial && sealedRevision ? pageCacheKey(id, sealedRevision, totalBytes) : undefined;
  const initialBody = (): LoadedBody => {
    const cached = sealedKey === undefined ? undefined : readPageCache(sealedKey);
    const text = cachedBody(cached, preview, totalBytes) ?? preview;
    return {
      bodyId,
      blocked: false,
      bytes: pageBytes(text),
      error: undefined,
      liveBytes: 0,
      projectionId,
      text,
    };
  };
  const [loaded, setLoaded] = useState(initialBody);
  const state = loaded.projectionId === projectionId ? loaded : initialBody();
  if (state !== loaded) setLoaded(state);
  const { bodyId: currentBody, bytes, text } = state;
  // A page can report a replacement live body before the projection does.
  // That body gets its own cursor, even if the old read also started at zero.
  const readId =
    currentBody === bodyId ? projectionId : JSON.stringify([...projectionIdentity, currentBody]);
  const announced = partial
    ? Math.max(state.liveBytes, currentBody === bodyId ? totalBytes : 0)
    : totalBytes;
  const complete = !partial && bytes >= announced;
  const more = !state.blocked && bytes < announced;
  useEffect(() => {
    if (complete && sealedKey !== undefined && text !== preview) writePageCache(sealedKey, text);
  }, [complete, preview, sealedKey, text]);

  const fetcher = useCallback(
    async (signal: AbortSignal, offset: number): Promise<void> => {
      // A layout effect may be replayed before the view paints in development.
      await Promise.resolve();
      if (signal.aborted) return;
      const updateCurrent = (update: (old: LoadedBody) => LoadedBody) => {
        setLoaded((old) =>
          !signal.aborted &&
          old.projectionId === projectionId &&
          old.bodyId === currentBody &&
          old.bytes === offset
            ? update(old)
            : old,
        );
      };
      const showError = (error: string) => updateCurrent((old) => ({ ...old, error }));
      const result = await window.aura.v1
        .readAgentMessageText({
          session_id: sessionId,
          source,
          offset,
          limit: MESSAGE_TEXT_PAGE_BYTES,
        })
        .catch((reason: unknown) => {
          showError(failedAction(reason, BODY_UNREAD, BODY_NEXT));
          throw reason;
        });
      if (signal.aborted) return;
      if (!result.ok) {
        showError(failedAction(result.error, BODY_UNREAD, BODY_NEXT));
        throw new Error(result.error.message);
      }
      if (!matchesSource(result.value.source, source)) {
        showError('正文来源已变化，请重新加载会话。');
        throw new Error('message text source changed');
      }
      const page = result.value;
      const pageBody = page.body_id ?? null;
      if (partial !== (pageBody !== null)) {
        // Wait for the projection when the read and the view disagree about sealing.
        updateCurrent((old) => ({ ...old, blocked: true }));
        return;
      }
      if (partial && (pageBody !== currentBody || (page.from_offset !== offset && offset > 0))) {
        // A different live body or a moved UTF-8 boundary requires a fresh prefix.
        updateCurrent((old) => ({
          ...old,
          bodyId: pageBody,
          bytes: 0,
          error: undefined,
          liveBytes: page.total_bytes,
          text: '',
        }));
        return;
      }
      if (
        page.from_offset !== offset ||
        (!partial &&
          (page.total_bytes !== totalBytes ||
            (sealedRevision !== null && page.sealed_revision !== sealedRevision)))
      ) {
        showError('正文来源已变化，请重新加载会话。');
        throw new Error('message text boundary changed');
      }
      const nextBytes = offset + pageBytes(page.text);
      if (nextBytes > page.total_bytes) {
        showError('正文超过声明的长度，请重新加载会话。');
        throw new Error('message text exceeded the announced length');
      }
      if (nextBytes === offset && offset < page.total_bytes) {
        showError('正文少于声明的长度，请重新加载会话。');
        throw new Error('message text ended before the announced length');
      }
      // Check the applied cursor inside the updater. Two responses can arrive
      // before React commits, both reporting the same valid request offset.
      updateCurrent((old) => ({
        ...old,
        bytes: nextBytes,
        error: undefined,
        liveBytes: page.total_bytes,
        text: old.text + page.text,
      }));
    },
    [currentBody, partial, projectionId, sealedRevision, sessionId, setLoaded, source, totalBytes],
  );
  const [visible, setVisible] = useState(false);
  const element = useRef<HTMLDivElement | null>(null);
  // The exact window to start a page in: the moment a new byte count renders
  // because the previous page arrived, and — for a running step — every time the
  // projection reports more text than is on screen. Registering the task passes
  // the byte the next page continues from and carries the viewport state, so a
  // page for a body the user is looking at never queues behind background work.
  useLayoutEffect(() => {
    if (!more || state.error !== undefined) return;
    messageTextQueue.read(readId, bytes, fetcher, visible);
    messageTextQueue.setVisible(readId, visible);
  }, [bytes, fetcher, readId, more, state.error, visible]);
  useLayoutEffect(() => () => messageTextQueue.remove(readId), [readId]);
  useEffect(() => {
    const target = element.current;
    if (!target) return;
    if (typeof IntersectionObserver !== 'function') {
      // Without the observer there is no viewport information: treat the body as
      // visible so it still fills in, just without priority ordering. There is
      // no external subscription to report this, so it is state set once here.
      // eslint-disable-next-line react-hooks/set-state-in-effect
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => {
        // Both directions are reported: a body that left the viewport gives up
        // its priority so off-screen pages cannot outrank what is on screen.
        const entry = entries.at(-1);
        if (entry) setVisible(entry.isIntersecting);
      },
      { rootMargin: VIEWPORT_MARGIN },
    );
    observer.observe(target);
    return () => observer.disconnect();
  }, []);
  const retry = () => {
    setLoaded((old) => ({ ...old, error: undefined }));
    messageTextQueue.retry(readId);
  };

  return {
    busy: !complete && !partial && state.error === undefined,
    element,
    error: state.error,
    more: !complete,
    retry,
    text,
  };
}

interface LoadedBody {
  /** Body ID returned by the reader, which can temporarily lead the projection. */
  bodyId: string | null;
  /** Wait for the projection when a read finds the live/sealed state has changed. */
  blocked: boolean;
  bytes: number;
  error: string | undefined;
  liveBytes: number;
  projectionId: string;
  text: string;
}
