import { describe, expect, it, vi } from 'vitest';
import type { AgentSessionsActivity } from '@aura/contracts/generated';

import {
  DEFAULT_NOTIFICATION_PREFERENCES,
  type NotificationPreferences,
} from '../../shared/notifications.js';
import { SessionNotifier, type SessionNotice } from './session-notifier.js';

const A = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAA';
const B = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAB';
const T1 = 'trn_01ARZ3NDEKTSV4RRFFQ69G5FA1';
const T2 = 'trn_01ARZ3NDEKTSV4RRFFQ69G5FA2';

function harness(options: { front?: string; preferences?: Partial<NotificationPreferences> } = {}) {
  let next: AgentSessionsActivity = { sessions: [] };
  const asked: string[][] = [];
  const shown: SessionNotice[] = [];
  const notifier = new SessionNotifier({
    read: vi.fn(async (request: { ended_turns: string[] }) => {
      asked.push(request.ended_turns);
      return next;
    }),
    preferences: () => ({ ...DEFAULT_NOTIFICATION_PREFERENCES, ...options.preferences }),
    inFront: (sessionId) => sessionId === options.front,
    notify: (notice) => shown.push(notice),
  });
  return {
    notifier,
    asked,
    shown,
    set: (activity: AgentSessionsActivity) => {
      next = activity;
    },
  };
}

const running = (session: string, turn: string) => ({
  session_id: session,
  state: 'running' as const,
  turn_task_ids: [turn],
});

describe('notifications about conversations', () => {
  it('say when a conversation starts waiting for an approval or an answer, once', async () => {
    const h = harness();
    h.set({ sessions: [running(A, T1)] });
    await h.notifier.poll();
    h.set({
      sessions: [{ ...running(A, T1), state: 'waiting', waiting_for: 'approval' }],
    });
    await h.notifier.poll();
    await h.notifier.poll();
    h.set({
      sessions: [{ ...running(A, T1), state: 'waiting', waiting_for: 'question' }],
    });
    await h.notifier.poll();
    expect(h.shown).toEqual([
      { sessionId: A, kind: 'approval' },
      { sessionId: A, kind: 'question' },
    ]);
  });

  it('ask how a turn that left ended, and say so for a finished or failed one', async () => {
    const h = harness();
    h.set({ sessions: [running(A, T1), running(B, T2)] });
    await h.notifier.poll();
    h.set({ sessions: [] });
    await h.notifier.poll();
    expect(h.asked.at(-1)).toEqual([]);
    h.set({
      sessions: [],
      ended: [
        { session_id: A, turn_task_id: T1, outcome: 'completed' },
        { session_id: B, turn_task_id: T2, outcome: 'failed' },
      ],
    });
    await h.notifier.poll();
    expect(h.asked.at(-1)).toEqual([T1, T2]);
    expect(h.shown).toEqual([
      { sessionId: A, kind: 'completed' },
      { sessionId: B, kind: 'failed' },
    ]);
    await h.notifier.poll();
    expect(h.asked.at(-1), 'each turn is asked about once').toEqual([]);
  });

  it('stay quiet about the conversation in front, a stop the user made, and what is off', async () => {
    const h = harness({ front: A, preferences: { completions: false } });
    h.set({ sessions: [running(A, T1), running(B, T2)] });
    await h.notifier.poll();
    h.set({
      sessions: [{ ...running(B, T2), state: 'waiting', waiting_for: 'approval' }],
      ended: [{ session_id: A, turn_task_id: T1, outcome: 'failed' }],
    });
    await h.notifier.poll();
    await h.notifier.poll();
    h.set({
      sessions: [],
      ended: [{ session_id: B, turn_task_id: T2, outcome: 'completed' }],
    });
    await h.notifier.poll();
    await h.notifier.poll();
    expect(h.shown).toEqual([{ sessionId: B, kind: 'approval' }]);
    const stopped = harness();
    stopped.set({ sessions: [running(A, T1)] });
    await stopped.notifier.poll();
    stopped.set({ sessions: [] });
    await stopped.notifier.poll();
    stopped.set({
      sessions: [],
      ended: [{ session_id: A, turn_task_id: T1, outcome: 'cancelled' }],
    });
    await stopped.notifier.poll();
    expect(stopped.shown).toEqual([]);
  });

  it('say nothing about what was already waiting when Aura started', async () => {
    const h = harness();
    h.set({ sessions: [{ ...running(A, T1), state: 'waiting', waiting_for: 'approval' }] });
    await h.notifier.poll();
    await h.notifier.poll();
    expect(h.shown).toEqual([]);
  });
});
