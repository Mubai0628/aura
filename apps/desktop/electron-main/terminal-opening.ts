import type {
  CancelDisposition,
  CorrelationId,
  TerminalOpeningChangeV1,
  TerminalOpeningPhaseV1,
  TerminalOpeningStartV1,
  TerminalViewV1,
} from '@aura/contracts/generated';
import type { CoreSupervisor } from './core-supervisor.js';
import { subscribeTerminalOpeningProgress } from './terminal-opening-progress.js';
import {
  isCancelDisposition,
  isConfirmedOpeningCancellation,
} from '../shared/terminal-opening-validation.js';
import { isRecord } from '../shared/projection-validation/common.js';

type OpeningCore = Pick<CoreSupervisor, 'requestAgent' | 'cancel' | 'snapshot'>;
interface Attempt {
  owner: object;
  core: OpeningCore;
  source: TerminalOpeningStartV1;
  runtime: string;
  request?: CorrelationId;
  cancelIfUnsent?: () => boolean;
  phase?: TerminalOpeningPhaseV1;
  cancelRequested: boolean;
  cancelSent: boolean;
  cancelPromise?: Promise<CancelDisposition>;
  resolveCancel?: (value: CancelDisposition) => void;
  rejectCancel?: (error: unknown) => void;
}

/** Window and opaque attempt token are the only renderer-selected cancellation scope. */
export class TerminalOpenings {
  readonly #owners = new WeakMap<object, Map<string, Attempt>>();
  constructor(private readonly publish: (owner: object, change: TerminalOpeningChangeV1) => void) {}
  async start(
    owner: object,
    source: TerminalOpeningStartV1,
    core: OpeningCore,
  ): Promise<TerminalViewV1> {
    let all = this.#owners.get(owner);
    if (!all) {
      all = new Map();
      this.#owners.set(owner, all);
    }
    if (all.has(source.opening_id)) throw new Error('terminal.opening.duplicate');
    if (all.size >= 16) throw new Error('terminal.opening.overloaded');
    const attempt: Attempt = {
      owner,
      source,
      core,
      runtime: core.snapshot.runtime_instance_id,
      cancelRequested: false,
      cancelSent: false,
    };
    all.set(source.opening_id, attempt);
    const detachProgress = subscribeTerminalOpeningProgress(core, (value) => {
      if (
        value.request_id !== attempt.request ||
        value.session_id !== source.request.session_id ||
        value.runtime_instance_id !== attempt.runtime
      )
        return;
      // Launch and cancellation are terminal choices of the Core gate, never reversible by stale data.
      if (attempt.phase === 'launching' || attempt.phase === 'cancelled') return;
      attempt.phase = value.phase;
      try {
        this.publish(owner, {
          contract_version: 1,
          opening_id: source.opening_id,
          runtime_instance_id: value.runtime_instance_id,
          session_id: value.session_id,
          phase: value.phase,
        });
      } catch {
        /* a closing renderer cannot prevent cancellation of its request */
      }
      if (value.phase === 'launching') attempt.resolveCancel?.('too_late_to_cancel');
      else if (value.phase === 'cancelled') attempt.resolveCancel?.('accepted');
      else if (attempt.cancelRequested) this.#sendCancellation(attempt);
    });
    let finalCancellation: CancelDisposition = 'unknown_request';
    try {
      const result = await core.requestAgent(
        'terminal.user.open',
        source.request,
        (id) => {
          attempt.request = id;
          if (attempt.cancelRequested) this.#sendCancellation(attempt);
        },
        (cancel) => {
          attempt.cancelIfUnsent = cancel;
          if (attempt.cancelRequested && cancel()) {
            attempt.phase = 'cancelled';
            attempt.resolveCancel?.('accepted');
          }
        },
      );
      finalCancellation = 'too_late_to_cancel';
      return result;
    } catch (error) {
      if (
        isRecord(error) &&
        isRecord(error.envelope) &&
        typeof error.envelope.code === 'string' &&
        typeof error.envelope.terminal === 'string' &&
        isConfirmedOpeningCancellation({
          code: error.envelope.code,
          terminal: error.envelope.terminal,
          details: isRecord(error.envelope.details) ? error.envelope.details : undefined,
        })
      )
        finalCancellation = 'accepted';
      throw error;
    } finally {
      detachProgress();
      all.delete(source.opening_id);
      attempt.resolveCancel?.(finalCancellation);
    }
  }
  cancel(owner: object, openingId: string): Promise<CancelDisposition> {
    const attempt = this.#owners.get(owner)?.get(openingId);
    if (!attempt) return Promise.resolve('unknown_request');
    if (attempt.phase === 'launching') return Promise.resolve('too_late_to_cancel');
    if (attempt.phase === 'cancelled') return Promise.resolve('accepted');
    if (attempt.cancelPromise) return attempt.cancelPromise;
    attempt.cancelRequested = true;
    attempt.cancelPromise = new Promise((resolve, reject) => {
      attempt.resolveCancel = resolve;
      attempt.rejectCancel = reject;
    });
    if (attempt.cancelIfUnsent?.()) {
      attempt.phase = 'cancelled';
      attempt.resolveCancel?.('accepted');
    }
    if (attempt.request) this.#sendCancellation(attempt);
    return attempt.cancelPromise;
  }
  closeOwner(owner: object): void {
    for (const id of this.#owners.get(owner)?.keys() ?? [])
      void this.cancel(owner, id).catch(() => {});
  }
  #sendCancellation(attempt: Attempt): void {
    if (attempt.cancelSent || !attempt.request) return;
    if (attempt.core.snapshot.runtime_instance_id !== attempt.runtime) {
      attempt.rejectCancel?.(new Error('terminal.opening.runtime_changed'));
      return;
    }
    attempt.cancelSent = true;
    void attempt.core
      .cancel(attempt.request)
      .then((value) => {
        if (!isCancelDisposition(value)) throw new Error('terminal.opening.invalid_cancel_reply');
        attempt.resolveCancel?.(value);
      })
      .catch((error: unknown) => attempt.rejectCancel?.(error));
  }
}
