import type {
  AgentSessionWaitingFor,
  AgentSessionsActivity,
  AgentTurnOutcome,
} from '@aura/contracts/generated';

import type { NotificationPreferences } from '../../shared/notifications.js';

export type SessionNoticeKind = 'approval' | 'question' | 'completed' | 'failed';

export interface SessionNotice {
  sessionId: string;
  kind: SessionNoticeKind;
}

const WAITING: Record<AgentSessionWaitingFor, SessionNoticeKind> = {
  approval: 'approval',
  question: 'question',
  context: 'question',
};

/** A stop the user made says nothing; an end the user did not see does. */
const ENDED: Record<AgentTurnOutcome, SessionNoticeKind | undefined> = {
  completed: 'completed',
  failed: 'failed',
  recovery_required: 'failed',
  cancelled: undefined,
};

const SWITCH: Record<SessionNoticeKind, keyof NotificationPreferences> = {
  approval: 'approvals',
  question: 'questions',
  completed: 'completions',
  failed: 'failures',
};

/** Turns already announced as ended are remembered this far back. */
const ANNOUNCED_KEPT = 256;

export interface SessionNotifierDeps {
  read(request: { ended_turns: string[] }): Promise<AgentSessionsActivity>;
  preferences(): NotificationPreferences;
  inFront(sessionId: string): boolean;
  notify(notice: SessionNotice): void;
}

/**
 * What conversations need or have done, from Core's activity read now and then: a conversation
 * that starts waiting for an approval or an answer, and a turn that ended. A turn that leaves the
 * activity is asked about by the next read, which says how it ended. Nothing is said about the
 * conversation in front, about what a switch turns off, or about what already waited at start.
 */
export class SessionNotifier {
  readonly #deps: SessionNotifierDeps;
  #first = true;
  #underWay = new Map<string, string>();
  #left = new Set<string>();
  #waiting = new Map<string, SessionNoticeKind>();
  readonly #announced = new Set<string>();

  constructor(deps: SessionNotifierDeps) {
    this.#deps = deps;
  }

  async poll(): Promise<void> {
    const asked = [...this.#left];
    const activity = await this.#deps.read({ ended_turns: asked });
    this.#left.clear();
    for (const ended of activity.ended ?? []) {
      if (this.#announced.has(ended.turn_task_id)) continue;
      this.#remember(ended.turn_task_id);
      const kind = ENDED[ended.outcome];
      if (kind !== undefined) this.#say({ sessionId: ended.session_id, kind });
    }
    const underWay = new Map<string, string>();
    const waiting = new Map<string, SessionNoticeKind>();
    for (const session of activity.sessions) {
      for (const turn of session.turn_task_ids ?? []) underWay.set(turn, session.session_id);
      const kind = session.waiting_for ? WAITING[session.waiting_for] : undefined;
      if (kind === undefined) continue;
      waiting.set(session.session_id, kind);
      if (!this.#first && this.#waiting.get(session.session_id) !== kind)
        this.#say({ sessionId: session.session_id, kind });
    }
    for (const turn of this.#underWay.keys()) if (!underWay.has(turn)) this.#left.add(turn);
    this.#underWay = underWay;
    this.#waiting = waiting;
    this.#first = false;
  }

  #say(notice: SessionNotice): void {
    if (!this.#deps.preferences()[SWITCH[notice.kind]]) return;
    if (this.#deps.inFront(notice.sessionId)) return;
    this.#deps.notify(notice);
  }

  #remember(turn: string): void {
    this.#announced.add(turn);
    if (this.#announced.size > ANNOUNCED_KEPT)
      this.#announced.delete(this.#announced.values().next().value!);
  }
}
