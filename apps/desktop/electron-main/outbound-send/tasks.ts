import type { QueuePriority } from '../frame-scheduler.js';
import type { SecretInputOperation } from '../protocol.js';
import {
  CONTROL_SOURCE_BUDGET_BYTES,
  P0_SOURCE_BUDGET_BYTES,
  STREAM_SOURCE_BUDGET_BYTES,
} from './budgets.js';

/** Outbound tasks, their outcomes and the SecretInput ownership. */
/** What this owner can honestly say about one task once it has ended. */
export interface OutboundTaskOutcome {
  /**
   * True when the task was taken back BEFORE any of its frames was built: the frame never existed,
   * spent no sequence number and never reached the sink, so `not_sent` is provable.
   */
  readonly withdrewBeforeWrite: boolean;
  /** True once at least one frame of this task was built and handed to the sink. */
  readonly wroteAnything: boolean;
  /** True when the task's own payload could not be built: nothing of it reached the sink. */
  readonly refusedBeforeWrite: boolean;
  /**
   * True when a complete-frame write could not finish inside its own budget. The connection may hold
   * half a frame, so its owner has to retire it and keep the outcome unknown.
   */
  readonly timedOutPhysically: boolean;
}

/** What a task is doing right now. */
export type OutboundTaskState =
  'registered' | 'queued' | 'writing' | 'written' | 'withdrawn' | 'refused' | 'failed';

/**
 * Ownership of ONE secret buffer across the send boundary.
 *
 * The caller owns the bytes until the send boundary takes them over. The takeover makes one bounded
 * copy, zeroes the caller's buffer at once, and from then on the sender alone reads the copy. That is
 * what makes a caller-side timeout harmless: it can clear its own (already empty) buffer, but never
 * bytes the sender is still using to build the frame it promised to send.
 */
export class SecretInputOwnership {
  #callerBytes: Buffer | undefined;
  #sendBytes: Buffer | undefined;

  constructor(callerBytes: Buffer) {
    this.#callerBytes = callerBytes;
  }

  /** True between the send-boundary takeover and the release of the send-owned copy. */
  get takenOver(): boolean {
    return this.#sendBytes !== undefined;
  }

  /** True while the caller's own buffer is still the one that will be copied. */
  get callerOwnsBytes(): boolean {
    return this.#callerBytes !== undefined;
  }

  /**
   * The send boundary takes the bytes over: ONE bounded copy for the atomic binding, caller buffer
   * zeroed at once. Only called while the request is still the channel's to send.
   */
  takeOver(): Buffer {
    const existing = this.#sendBytes;
    if (existing !== undefined) return existing;
    const callerBytes = this.#callerBytes;
    if (callerBytes === undefined) {
      throw new Error('SecretInput bytes were already released by their owner');
    }
    const copy = Buffer.from(callerBytes);
    callerBytes.fill(0);
    this.#callerBytes = undefined;
    this.#sendBytes = copy;
    return copy;
  }

  /** Zero whatever this holder still owns. Safe to call on every exit path, and idempotent. */
  release(): void {
    this.#callerBytes?.fill(0);
    this.#callerBytes = undefined;
    this.#sendBytes?.fill(0);
    this.#sendBytes = undefined;
  }
}

/** Everything the owner needs to know about one request it is about to send. */
export interface OutboundRequestSpec {
  readonly commandInvocationId: string | undefined;
  readonly correlationId: string;
  /** The exact JSON-RPC frame the Core must see, already serialised once by the caller. */
  readonly framePayload: Buffer;
  /**
   * The invocation name, retained so the transport can report the send stage of THIS request when its
   * frame really begins writing. A method name, never its params.
   */
  readonly method?: string;
  readonly priority: QueuePriority;
  readonly secret?: {
    readonly operation: SecretInputOperation;
    readonly ownership: SecretInputOwnership;
  };
  /** A label for diagnostics only. It never carries params or a secret. */
  readonly taskLabel?: string;
  /** See `FrameRequestBase.wanted`: may the FIRST frame of this task still be built? */
  readonly wanted?: () => boolean;
}

/** What the transport must do when one task ends in a way it cannot decide alone. */
export interface OutboundSendOwnerHandlers {
  /**
   * What the transport must do when one task ends in a way it cannot decide alone.
   *
   * Every handler names the task by the handle its registrant created, so a task that settles
   * synchronously is still attributable to the request that registered it.
   */
  readonly onWriteStarted: (task: OutboundTaskKey) => void;
  /**
   * THIS connection can no longer continue honestly: a complete-frame write stopped progressing with
   * a frame possibly half written, or a SecretInput binding could not be completed after its command
   * frame was committed. The transport retires the connection exactly once.
   */
  readonly onUnrecoverable: (reason: Error) => void;
  /**
   * The task ended. `reason` is what a caller may be told; `connectionUnrecoverable` is the owner's
   * own verdict about the CHANNEL. The transport takes its single settlement decision here.
   */
  readonly onSettled: (task: OutboundTaskKey, ending: OutboundTaskEnding) => void;
}

/** How one task ended, as the transport needs to hear it. */
export interface OutboundTaskEnding {
  readonly connectionUnrecoverable: boolean;
  readonly outcome: OutboundTaskOutcome;
  readonly reason: Error | undefined;
}

/**
 * A stable registration handle for one task.
 *
 * WHY IT EXISTS: a task may end synchronously - a frame that is simply written immediately, or a
 * payload refused before any byte is written - and the caller has to be able to recognise WHICH task
 * ended from the moment it asks for one. The handle is therefore created by the CALLER and handed in,
 * so it already carries the request identity before anything can settle.
 */
export interface OutboundTaskKey {
  readonly label: string;
}

/** Create the registration handle for one task. */
export const createTaskKey = (label: string): OutboundTaskKey => ({ label });

/** What the send owner hands back for one registered task. */
export interface OutboundTask {
  /**
   * Settles when this task is no longer the send owner's business: written, withdrawn, refused or
   * failed. It never rejects - a send failure is not a caller's error, and the transport's single
   * settlement path decides how the CALLER hears about it.
   */
  readonly done: Promise<void>;
  /** How the task ended. Meaningful once `done` has settled. */
  outcome(): OutboundTaskOutcome;
  /** The error that ended this task, if any. Set once `done` has settled. */
  failure(): Error | undefined;
  /** True once this task has ended, whichever way it ended. */
  settled(): boolean;
  /**
   * Release every real resource this task still holds: its secret copy, its byte reservation and its
   * timers. Idempotent, and safe to call while the task is still live - a retired channel must not
   * wait out a deadline to drop a record it can release at once.
   */
  release(): void;
  readonly state: () => OutboundTaskState;
  /**
   * Asked once per frame at the scheduling boundary, before the frame is built.
   *
   * This is the ONLY way a late frame is stopped: a request whose waiter was already rejected must not
   * have its bytes dispatched afterwards with nobody left to hear about it, and `not_sent` would be a
   * lie if they were. The owner answers from live state, so the decision is taken at the moment the
   * frame would really go out rather than when it was queued: `setWanted` changes the scheduler's
   * actual decision, because the scheduler calls back into this record rather than into a snapshot.
   */
  setWanted(wanted: () => boolean): void;
  /** Take a task back before any of its frames was built. False once a frame has been built. */
  withdraw(): boolean;
}

export interface TaskRecord {
  /**
   * How many frames one atomic binding of this task holds, or 0 when it is not an atomic binding.
   * It is what decides whether a whole-group deadline exists at all.
   */
  readonly atomicFrames: number;
  /** Which source class this task's reservation is measured against, for its whole life. */
  readonly sourceClass: SourceClass;
  readonly done: Promise<void>;
  readonly key: OutboundTaskKey;
  readonly method: string | undefined;
  readonly secretOwnership: SecretInputOwnership | undefined;
  /** Give the scheduler's unbuilt frame declarations back at once. Set by `#enqueue`. */
  abandonHeld: (() => boolean) | undefined;
  doneResolved: boolean;
  /** The error that ended this task, when it ended as a failure rather than a completion. */
  failure: Error | undefined;
  /**
   * Frames of this task whose write promise has NOT settled yet. It is what makes the working-byte
   * reservation exactly-once: taken before the sink call, given back by the real write promise.
   */
  framesInFlight: number;
  /** Set when ONE complete-frame write did not settle inside its own budget. */
  physicallyStuck: boolean;
  priority: QueuePriority;
  /** SOURCE bytes this task holds for its whole life; given back by `#release`. */
  reservedBytes: number;
  /** Source builders remain owned until the scheduler's actual group completion. */
  schedulerSettled: boolean;
  resolveDone: () => void;
  state: OutboundTaskState;
  /** The handle this task's owner sees, set as soon as it is created. */
  task: OutboundTask | undefined;
  /** How long the frame that is waiting right now may still wait for the wire. */
  queueTimer: NodeJS.Timeout | undefined;
  /** How long the frame that is inside the sink right now may still take. */
  frameTimer: NodeJS.Timeout | undefined;
  /** How long the atomic binding as a whole may still take, counted from frame one's dispatch. */
  groupTimer: NodeJS.Timeout | undefined;
  /** Asked once per frame at the scheduling boundary; defaults to "this task still wants its frame". */
  wanted: (() => boolean) | undefined;
  wantedContinuation: (() => boolean) | undefined;
  withdrawNow: (() => boolean) | undefined;
  /** Frames really handed to the sink. */
  writtenFrames: number;
}

/** The three source classes. Each has its own budget, so one class cannot refuse another. */
export type SourceClass = 'p0' | 'control' | 'stream';

export const SOURCE_LIMITS: Readonly<Record<SourceClass, number>> = {
  control: CONTROL_SOURCE_BUDGET_BYTES,
  p0: P0_SOURCE_BUDGET_BYTES,
  stream: STREAM_SOURCE_BUDGET_BYTES,
};

export const asError = (error: unknown): Error =>
  error instanceof Error ? error : new Error(String(error));

/** Counts and byte totals only: how many tasks this owner ran, how they ended, and what it holds. */
export interface OutboundSendOwnerFacts {
  /**
   * WHOLE-CHANNEL RETAINED-BYTE BOUND, and what is retained against it right now.
   *
   * Its scope is exactly `MAX_RETAINED_SOURCE_BYTES + MAX_FRAME_WORKING_BYTES`: every source byte this
   * owner keeps for a live task, plus the one encoded frame inside the sink and the native copy of it.
   * It is NOT a file-size cap and NOT a whole-process memory bound: the caller that read a file into a
   * Buffer still holds its own reference to those bytes.
   */
  readonly maxPendingBytes: number;
  readonly pendingBytes: number;
  readonly peakPendingBytes: number;
  /** The SOURCE scope on its own: retained source bytes, their bound, and the peak reached. */
  readonly maxSourceBytes: number;
  readonly peakSourceBytes: number;
  readonly sourceBytes: number;
  /** The source bytes each class holds right now. Their sum is `sourceBytes`. */
  readonly p0SourceBytes: number;
  readonly controlSourceBytes: number;
  readonly streamSourceBytes: number;
  /**
   * The FRAME WRITE WORKING scope on its own: the encoded frame inside the sink, the copy the native
   * addon holds for the same write, their shared bound and the peaks reached.
   */
  readonly frameBytes: number;
  readonly maxFrameWorkingBytes: number;
  readonly nativeBytes: number;
  readonly peakFrameBytes: number;
  readonly peakNativeBytes: number;
  /** The deadline of ONE complete-frame write, from handoff to sink until its promise settles. */
  readonly frameWriteBudgetMs: number;
  /** Kept under its historical name; it names the same complete-frame write budget. */
  readonly physicalSendBudgetMs: number;
  readonly physicallyStuck: number;
  /** How long ONE frame may wait for the wire before it is dropped, provably unsent. */
  readonly queueWaitBudgetMs: number;
  readonly refusalsOverBudget: number;
  readonly refusedBeforeWrite: number;
  /** How long the WHOLE atomic binding may take, counted from its command frame's dispatch. */
  readonly secretInputBindingBudgetMs: number;
  readonly tasks: number;
  readonly watching: number;
  readonly withdrewBeforeWrite: number;
  readonly wroteAnything: number;
}
