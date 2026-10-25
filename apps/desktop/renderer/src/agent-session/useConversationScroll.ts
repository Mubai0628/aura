import type { RefObject } from 'react';
import { useEffect, useLayoutEffect, useRef, useState } from 'react';

/** How close to the bottom still counts as following the conversation. */
const FOLLOW_SLACK = 90;

/**
 * Keeps the conversation at its last message while the reader follows it.
 *
 * Every switch to a conversation opens it at its last message. Positions
 * used to be restored per Core runtime, which every conversation shares, so a switch kept the old
 * position; at the top, the "earlier messages" sentinel then loaded page after page up to the
 * first message.
 */
export function useConversationScroll(
  viewportRef: RefObject<HTMLDivElement | null>,
  session: string,
  runtime: string | undefined,
  contentVersion: string,
) {
  const following = useRef(true);
  const shown = useRef<string | undefined>(undefined);
  const previousContent = useRef(contentVersion);
  const [hasNew, setHasNew] = useState(false);
  useLayoutEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport || !runtime) return;
    const key = `${session}:${runtime}`;
    if (shown.current !== key) {
      shown.current = key;
      following.current = true;
      setHasNew(false);
    }
    if (following.current) viewport.scrollTop = viewport.scrollHeight;
  }, [viewportRef, session, runtime, contentVersion]);
  useEffect(() => {
    if (previousContent.current !== contentVersion && !following.current) setHasNew(true);
    previousContent.current = contentVersion;
  }, [contentVersion]);
  useEffect(() => {
    const viewport = viewportRef.current;
    if (!viewport || !runtime) return;
    const save = () => {
      following.current =
        viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight < FOLLOW_SLACK;
      if (following.current) setHasNew(false);
    };
    viewport.addEventListener('scroll', save, { passive: true });
    // Content that grows after it is shown (tool rows, code, images) keeps a follower at the end.
    const resized =
      typeof ResizeObserver === 'undefined'
        ? undefined
        : new ResizeObserver(() => {
            if (following.current) viewport.scrollTop = viewport.scrollHeight;
          });
    for (const child of viewport.children) resized?.observe(child);
    return () => {
      viewport.removeEventListener('scroll', save);
      resized?.disconnect();
    };
  }, [viewportRef, session, runtime, contentVersion]);
  return {
    stopFollowing: () => {
      following.current = false;
    },
    hasNew,
    follow: () => {
      following.current = true;
      setHasNew(false);
      if (viewportRef.current) viewportRef.current.scrollTop = viewportRef.current.scrollHeight;
    },
  };
}
