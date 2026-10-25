import type { AgentMessageTextSource } from '@aura/contracts/generated';
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { MarkdownBlocks } from '../markdown-body';
import { userMessage } from './user-message';
import { pageCacheKey, readPageCache, writePageCache } from './message-text-cache';
import { messageTextQueue } from './message-text-queue';
import { MentionContext, mentionOf } from '../files/mention-context';

/** Roles that carry a real answer, a real person's words or the model's own process notes
 *  are Markdown. Model reasoning and program-made tool briefs stay plain text. */
export type MessageTextRole = 'user' | 'final' | 'reasoning' | 'process' | 'brief';

const PLAIN_ROLES: readonly MessageTextRole[] = ['reasoning', 'brief'];

/** How many bytes one automatic page reads: small enough to paint quickly,
 *  large enough that a long body arrives in a few passes. */
export const MESSAGE_TEXT_PAGE_BYTES = 32_768;

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
          <MarkdownBlocks streaming={streaming} text={text} />
        ) : (
          <MarkdownBlocks className={body} streaming={streaming} text={text} />
        )}
      </div>
      {error ? (
        <p role="alert">
          {userMessage(error)}
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
  const id = useMemo(() => queueId(sessionId, source), [sessionId, source]);
  // A new projection identity also owns a new queue task. Late pages of the
  // previous revision are aborted before they can append to the new preview.
  const readId = JSON.stringify([
    id,
    partial,
    bodyId,
    sealedRevision,
    preview,
    partial ? null : totalBytes,
  ]);
  const sealedKey =
    !partial && sealedRevision ? pageCacheKey(id, sealedRevision, totalBytes) : undefined;
  const [loaded, setLoaded] = useState<{
    /** The body the pages on screen were read from. */
    bodyId: string | null;
    sealedRevision: string | null;
    error: string | undefined;
    /** The announced size the projection reported when this identity began. */
    expectedBytes: number;
    id: string;
    /** The body the projection reported when this identity began. It decides
     *  when the projection has moved to another body, which is a different
     *  question from which body these pages came from: a reader can be ahead of
     *  the projection and holding a body the projection has not reported yet. */
    observed: string | null;
    partial: boolean;
    preview: string;
    text: string;
    /** The live size of the body after the last page, for a running step. */
    liveBytes: number;
    /** True once a sealed body proved it was read to its announced length. */
    exhausted: boolean;
    /** The projection still describes a body this reader cannot page: reading
     *  stops until the projection reports the body that really exists. */
    blocked: boolean;
  }>(() => ({
    bodyId,
    sealedRevision,
    blocked: false,
    error: undefined,
    exhausted: false,
    expectedBytes: totalBytes,
    id,
    liveBytes: 0,
    observed: bodyId,
    partial,
    preview,
    text:
      sealedKey === undefined
        ? preview
        : (cachedBody(readPageCache(sealedKey), preview, totalBytes) ?? preview),
  }));
  // The identity of a body is its immutable source, the preview the projection
  // currently reports, whether it is still running, and the body the projection
  // names. A preview of the same length but different text is a different body,
  // so pages already read must not be kept. Sealing does not keep a live suffix
  // just because it starts with the new preview.
  const staleIdentity =
    loaded.id !== id ||
    loaded.preview !== preview ||
    loaded.partial !== partial ||
    loaded.observed !== bodyId ||
    loaded.sealedRevision !== sealedRevision ||
    // A sealed body's announced size is part of its identity; a running step's
    // size only grows, and that is not a new body.
    (!partial && loaded.expectedBytes !== totalBytes);
  if (staleIdentity) {
    const remembered = sealedKey === undefined ? undefined : readPageCache(sealedKey);
    setLoaded({
      bodyId,
      sealedRevision,
      blocked: false,
      error: undefined,
      exhausted: false,
      expectedBytes: totalBytes,
      id,
      liveBytes: 0,
      observed: bodyId,
      partial,
      preview,
      text: cachedBody(remembered, preview, totalBytes) ?? preview,
    });
  }
  const state = staleIdentity ? undefined : loaded;
  const content = state?.text ?? preview;
  const bytes = pageBytes(content);
  // How long the body is known to be. A sealed body announced one size and that
  // promise is what "complete" means. A running step is measured by the pages
  // that were actually read, and — while those pages describe the body the
  // projection names — by the projection's own size, which is what tells this
  // component that the body has grown again since the last page.
  const announced = partial
    ? (state?.bodyId ?? null) === bodyId
      ? Math.max(state?.liveBytes ?? 0, totalBytes)
      : (state?.liveBytes ?? 0)
    : (state?.expectedBytes ?? totalBytes);
  // A sealed body is complete when everything it announced is on screen. A
  // running step is never "complete"; it has more to read while the body is
  // longer than what is displayed.
  const complete = !partial && bytes >= announced;
  const more = state?.blocked ? false : partial ? announced > bytes : !complete;
  const fetcher = useCallback(
    async (signal: AbortSignal, offset: number): Promise<void> => {
      const result = await window.aura.v1.readAgentMessageText({
        session_id: sessionId,
        source,
        offset,
        limit: MESSAGE_TEXT_PAGE_BYTES,
      });
      if (signal.aborted) return;
      if (!result.ok) {
        setLoaded((old) => ({ ...old, error: result.error.message }));
        throw new Error(result.error.message);
      }
      if (!matchesSource(result.value.source, source)) {
        setLoaded((old) => ({ ...old, error: '正文来源已变化，请重新加载会话。' }));
        throw new Error('message text source changed');
      }
      const page = result.value;
      if (partial) {
        if (page.body_id == null) {
          // The step finished while this reader was paging it. The sealed text is
          // authoritative and need not equal the live bytes, so nothing more is
          // read from here: the projection will report the sealed source and this
          // component reads that from its own preview.
          setLoaded((old) => ({ ...old, blocked: true }));
          return;
        }
        if (page.body_id !== (state?.bodyId ?? null)) {
          // The body restarted under this reader (a retry began). Its bytes are
          // not a prefix of the new body, so nothing read so far survives; the
          // next page starts from the first byte of the new body.
          setLoaded((old) => ({
            ...old,
            blocked: false,
            bodyId: page.body_id ?? null,
            error: undefined,
            liveBytes: page.total_bytes,
            text: '',
          }));
          return;
        }
        if (page.from_offset !== offset) {
          // The page does not continue the bytes on screen: read the body again
          // from its start rather than splice unrelated text together.
          setLoaded((old) => ({ ...old, error: undefined, liveBytes: page.total_bytes, text: '' }));
          return;
        }
        setLoaded((old) => ({
          ...old,
          blocked: false,
          error: undefined,
          liveBytes: page.total_bytes,
          text: old.text + page.text,
        }));
        return;
      }
      if (page.body_id != null) {
        // The projection said the step was done, but the Core answers from a
        // live body: the sealed text may differ, so wait for the sealed source
        // instead of appending live bytes to a sealed answer.
        setLoaded((old) => ({ ...old, blocked: true }));
        return;
      }
      if (
        page.total_bytes !== totalBytes ||
        (sealedRevision !== null && page.sealed_revision !== sealedRevision) ||
        page.from_offset !== offset
      ) {
        setLoaded((old) => ({ ...old, error: '正文来源已变化，请重新加载会话。' }));
        throw new Error('message text total changed');
      }
      if (page.text === '' && (page.next_offset ?? null) === null) {
        // The source reports itself exhausted. That is only believable when the
        // bytes already read really add up to the announced size; a short body
        // with an empty final page must never be recorded as complete, because
        // that would silently shorten the answer and hide the missing remainder.
        if (offset >= totalBytes) {
          setLoaded((old) => ({ ...old, error: undefined, exhausted: true }));
          return;
        }
        const detail = '正文少于声明的长度，请重新加载会话。';
        setLoaded((old) => ({ ...old, error: detail }));
        throw new Error('message text ended before the announced length');
      }
      setLoaded((old) => {
        const text = old.text + page.text;
        // Only a complete body whose pages agree with the recorded revision
        // enters the cache. An incomplete or live suffix is never reusable.
        if (
          sealedKey !== undefined &&
          !partial &&
          page.body_id == null &&
          text.startsWith(preview) &&
          page.sealed_revision === sealedRevision &&
          pageBytes(text) === totalBytes
        ) {
          writePageCache(sealedKey, text);
        }
        return { ...old, error: undefined, text };
      });
    },
    [partial, preview, sealedKey, sealedRevision, sessionId, source, state?.bodyId, totalBytes],
  );
  const [visible, setVisible] = useState(false);
  const element = useRef<HTMLDivElement | null>(null);
  // The exact window to start a page in: the moment a new byte count renders
  // because the previous page arrived, and — for a running step — every time the
  // projection reports more text than is on screen. Registering the task passes
  // the byte the next page continues from and carries the viewport state, so a
  // page for a body the user is looking at never queues behind background work.
  useLayoutEffect(() => {
    if (!more || state?.error !== undefined || state?.blocked) return;
    messageTextQueue.read(readId, bytes, fetcher, visible);
    messageTextQueue.setVisible(readId, visible);
  }, [bytes, fetcher, readId, more, state?.blocked, state?.error, visible]);
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
  const retry = useCallback(() => {
    setLoaded((old) => ({ ...old, error: undefined }));
    messageTextQueue.retry(readId);
  }, [readId]);

  return {
    busy: !complete && !partial && state?.error === undefined,
    element,
    error: state?.error,
    more: !complete,
    retry,
    text: content,
  };
}
