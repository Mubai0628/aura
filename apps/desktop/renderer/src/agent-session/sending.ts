import { useSyncExternalStore } from 'react';
import type { AgentSessionProjection } from '@aura/contracts/generated';

/** A message on its way to Core, shown in its conversation from the moment it is sent. */
export interface Sending {
  text: string;
  /** Names of the pictures and files sent with it. */
  files: string[];
  /** Core accepted it; it goes once the conversation shows it. */
  accepted: boolean;
}

/** How long an accepted message waits to appear before it stops being shown here. */
const SHOWN_FOR_MS = 15_000;

const pending = new Map<string, Sending>();
const listeners = new Set<() => void>();
const changed = () => listeners.forEach((listener) => listener());

/** Shows `sending` in the conversation `key` names: a local draft's ID or a session's. */
export function startSending(key: string, sending: Omit<Sending, 'accepted'>): void {
  pending.set(key, { ...sending, accepted: false });
  changed();
}

export function endSending(key: string): void {
  if (pending.delete(key)) changed();
}

/** Core accepted the message; it keeps showing until the conversation has it. */
export function acceptSending(key: string): void {
  const sending = pending.get(key);
  if (!sending) return;
  const accepted = { ...sending, accepted: true };
  pending.set(key, accepted);
  changed();
  setTimeout(() => {
    if (pending.get(key) === accepted) endSending(key);
  }, SHOWN_FOR_MS);
}

export function useSending(key: string | undefined): Sending | undefined {
  return useSyncExternalStore(
    (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    () => (key ? pending.get(key) : undefined),
  );
}

const normalized = (text: string) => text.replace(/\s+/gu, ' ').trim();

/** Whether the conversation already shows the message: a newer turn or a queued message has it. */
export function shownIn(projection: AgentSessionProjection | undefined, sending: Sending): boolean {
  if (!projection) return false;
  const text = normalized(sending.text);
  const matches = (preview: string) => {
    const shown = normalized(preview);
    return shown === text || (shown.length > 0 && text.startsWith(shown));
  };
  const latest = projection.turns.at(-1);
  return (
    (latest?.user_messages.some((message) => matches(message.text)) ?? false) ||
    (projection.inbox?.messages.some((message) => matches(message.text)) ?? false)
  );
}

export function resetSendingForTests(): void {
  pending.clear();
  changed();
}
