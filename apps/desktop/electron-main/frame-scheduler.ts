/**
 * Frame scheduling for ONE authenticated Core channel.
 *
 * WHY THIS IS ITS OWN MODULE. Three responsibilities were sharing `protocol.ts`: the wire codec
 * (encode/decode), the frame scheduler, and a `PriorityFrameWriter` that no production path used any
 * more. Keeping them in one file hid the fact that the scheduler is what decides when a frame becomes
 * the next bytes on the wire - and made it impossible to see which of the two writers a test was
 * really exercising. The codec stays in `protocol.ts`; scheduling lives here; frame preparation and
 * per-send budgets live in `outbound-send-owner.ts`.
 *
 * WHAT THIS SCHEDULER OWNS: the order complete frames reach the sink, the outbound sequence boundary,
 * queue and byte capacity, and the honest refusal of a frame that is taken back before it is built.
 * It does NOT own the connection, the request ledger or settlement.
 *
 * TWO STRUCTURES, DELIBERATELY DIFFERENT
 *
 *  1. An ATOMIC GROUP is a fixed set of at most `MAX_FRAMES_PER_GROUP` complete frames - in practice
 *     "command control frame + SecretInput frame". It may wait for a place in a queue, but once it has
 *     started it holds the scheduling turn until every one of its frames has been written, or until
 *     the channel retires: no other frame may be inserted between two frames of the binding. Its
 *     frames are all resident from the moment it is admitted, so its whole cost is declared and
 *     bounded up front.
 *
 *  2. A STREAM CURSOR is a sequence whose total length is NOT known to this scheduler and is NOT
 *     limited by the atomic two-frame ceiling. It exposes exactly ONE frame at a time (`first`, then
 *     whatever `next()` returns after the previous chunk really settled) and returns the turn at every
 *     COMPLETE-FRAME boundary, so a waiting P0 control frame simply becomes the next frame chosen.
 *
 * THE SEQUENCE NUMBER IS COMMITTED AT SELECTION. `build` runs once, immediately before the frame is
 * handed to the sink, so the numbers a channel sends are contiguous in true send order even when
 * frames are withdrawn, reordered by priority, or refused by their own validation.
 */
import type { Buffer } from 'node:buffer';
import {
  BULK_PRIORITIES,
  FRAME_SCHEDULER_LIMITS,
  type FrameSchedulerFacts,
  type FrameSchedulerLimits,
  type Occupancy,
  type Placement,
  SHARED_PRIORITIES,
  SHARED_SCHEDULE,
  type ScheduledGroup,
  type WriteOutcome,
  bucketTotal,
  frameBytes,
  freshBuckets,
  sumIn,
} from './frame-scheduler/limits.js';
import {
  type AsyncFrameSink,
  FrameBuildError,
  type FrameEnqueueRequest,
  FrameQueueClosedError,
  FrameTooLargeError,
  FrameWithdrawnError,
  FrameWriteAbortedError,
  MAX_FRAMES_PER_GROUP,
  type QueuePriority,
  type ScheduledFrame,
  p0FrameIsOversized,
} from './frame-scheduler/requests.js';
export {
  MAX_P0_FRAME_BYTES,
  p0FrameIsOversized,
  MAX_FRAMES_PER_GROUP,
  FrameBuildError,
  FrameTooLargeError,
  FrameQueueClosedError,
  FrameWriteAbortedError,
  FrameWithdrawnError,
} from './frame-scheduler/requests.js';
export type {
  AsyncFrameSink,
  QueuePriority,
  ScheduledFrame,
  FrameWanted,
  AtomicFrameGroupRequest,
  FrameStreamRequest,
  FrameEnqueueRequest,
} from './frame-scheduler/requests.js';
export { FRAME_SCHEDULER_LIMITS } from './frame-scheduler/limits.js';
export type { FrameSchedulerLimits, FrameSchedulerFacts } from './frame-scheduler/limits.js';

/**
 * The scheduler that gives a physical send its own owner.
 *
 * WHAT IT REPLACES: an outbound barrier that serialised a WHOLE logical send - every chunk of a bulk
 * stream included - behind one physical write promise. Under that shape a control frame could only be
 * considered after the entire stream had been written, so the writer's P0 priority was decided too
 * late to matter, and a frame whose write never finished had no owner with a budget of its own.
 *
 * WHAT IT DOES INSTEAD: every STREAM frame is its own queue entry, so the scheduling boundary between
 * two complete frames is real - a queued P0 control frame is simply the next frame chosen once the
 * current one has been written. A frame being written is never interrupted: selection happens
 * strictly between writes. An ATOMIC group writes its frames back to back with nothing scheduled in
 * between, which is what keeps the command/SecretInput binding atomic without capping how many
 * chunks a stream may have.
 *
 * WHAT IT DOES NOT OWN: the connection. A sink failure is recorded in `failureCause` and the queue is
 * closed; the channel owner decides what that means for its connection.
 */
export class FrameScheduler {
  readonly #sink: AsyncFrameSink;
  readonly #limits: FrameSchedulerLimits;
  /** Frames holding a place in a queue, per priority class. */
  readonly #queued: Occupancy[] = freshBuckets();
  /**
   * Tasks waiting for a place in a queue, one list per class.
   *
   * A plain list is what makes the bound real: a task that cannot be queued yet is one entry, not a
   * closure parked on a promise, and it leaves the list the moment it is queued or taken back. Its
   * frames are real memory too, which is why `#pending` is accounted for exactly like a queue.
   */
  readonly #pendingGroups: [
    ScheduledGroup[],
    ScheduledGroup[],
    ScheduledGroup[],
    ScheduledGroup[],
  ] = [[], [], [], []];
  readonly #pending: Occupancy[] = freshBuckets();
  /** Stream cursors between two of their own chunks: one frame each, held out of every queue. */
  readonly #stagedGroups = new Set<ScheduledGroup>();
  readonly #staged: Occupancy[] = freshBuckets();
  /** The frames holding a place in a queue, in scheduling order, per class. */
  readonly #queues: [ScheduledGroup[], ScheduledGroup[], ScheduledGroup[], ScheduledGroup[]] = [
    [],
    [],
    [],
    [],
  ];
  #bytesWritten = 0;
  #closed = false;
  #failureCause: Error | undefined;
  /** The frame currently handed to the sink, if any. It is NOT queue space: it is memory in use. */
  #inFlightBytes = 0;
  /** Frames of the selected group not written yet, excluding the one in the sink. */
  #inFlightOwedBytes = 0;
  #inFlightOwedFrames = 0;
  #inFlightPriority: QueuePriority | undefined;
  #nextSequence = 0n;
  #onSinkFailure: ((reason: Error) => void) | undefined;
  #peakCommittedBytes = 0;
  #peakHeldBytes = 0;
  #peakHeldFrames = 0;
  #peakPendingBytes = 0;
  #peakPendingGroups = 0;
  #peakQueuedBytes = 0;
  #peakQueuedFrames = 0;
  #peakQueuedGroups = 0;
  #peakStagedBytes = 0;
  #pumping = false;
  #scheduleCursor = 0;
  #withdrawnGroups = 0;
  #writesFailed = 0;
  #writesStarted = 0;
  #writesSucceeded = 0;

  constructor(sink: AsyncFrameSink, limits: FrameSchedulerLimits = FRAME_SCHEDULER_LIMITS) {
    this.#sink = sink;
    this.#limits = limits;
  }

  /**
   * Set only when the SINK failed a write: from that point part of a frame may already be on the
   * wire, so the framed byte stream can no longer be trusted. A scheduler closed by its owner
   * (channel retirement, a refused oversized frame) leaves this undefined, which is what lets the
   * channel owner tell a corrupt stream apart from a frame that was simply never written.
   */
  get failureCause(): Error | undefined {
    return this.#failureCause;
  }

  /** Called once when the sink fails a write, before the queue is closed. */
  onSinkFailure(handler: ((reason: Error) => void) | undefined): void {
    this.#onSinkFailure = handler;
  }

  /** The sequence number the next written frame will carry. */
  get nextSequence(): bigint {
    return this.#nextSequence;
  }

  /**
   * Queue one frame group. Resolves once every frame of the group has been written.
   *
   * The returned handle withdraws a group that has NOT been selected yet, and abandons a group whose
   * remaining frames will never be built. That is the honest way to take back a task nobody is waiting
   * for any more: nothing unbuilt was ever numbered, and nothing unbuilt stays resident.
   */
  enqueue(request: FrameEnqueueRequest): {
    /**
     * Drop every frame this group still holds UNBUILT and stop it being selected, without claiming any
     * of it was written. Unlike `withdraw` it also works for a group that has already written a frame
     * and still owes another one - a stream cursor abandoned between two complete chunks - where the
     * held frame is a declaration over bytes its owner is about to give back. It is a no-op (and is not
     * counted as a withdrawal) for a group that holds nothing any more, which is what every already
     * finished or already withdrawn group is.
     */
    readonly abandon: () => boolean;
    readonly done: Promise<void>;
    /**
     * Take the group back BEFORE any of its frames is built. False once a frame has been handed to the
     * sink: an atomic binding that has committed its command frame owes the rest of itself.
     */
    readonly withdraw: () => boolean;
  } {
    const frames = request.kind === 'atomic' ? [...request.frames] : [request.first];
    // Enforced structurally by the type AND checked here: a caller that reaches this with an atomic
    // group larger than the binding is refused before anything of it exists.
    if (request.kind === 'atomic' && frames.length > MAX_FRAMES_PER_GROUP) {
      throw new RangeError(`an atomic frame group may hold at most ${MAX_FRAMES_PER_GROUP} frames`);
    }
    if (request.priority === 0 && frames.some((frame) => p0FrameIsOversized(frame.sizeBytes))) {
      return {
        abandon: () => false,
        done: Promise.reject(
          new FrameTooLargeError('P0 frame exceeds its 32 KiB emergency frame limit'),
        ),
        withdraw: () => false,
      };
    }
    const group: ScheduledGroup = {
      frameIndex: 0,
      frames,
      kind: request.kind,
      label: request.label,
      next: request.kind === 'stream' ? request.next : undefined,
      onFrameQueued: request.onFrameQueued,
      onFrameWriteSettled: request.onFrameWriteSettled,
      onFrameWriteStarted: request.onFrameWriteStarted,
      priority: request.priority,
      reject: () => undefined,
      resolve: () => undefined,
      wanted: request.wanted,
      wantedContinuation: request.kind === 'stream' ? request.wantedContinuation : undefined,
      withdrawn: false,
    };
    const done = new Promise<void>((resolve, reject) => {
      group.resolve = resolve;
      group.reject = reject;
    });
    // Every frame of this group is now its owner's resident business and is waiting for the wire, so
    // each one starts its own QUEUE deadline here: an atomic group declares all of its frames at once,
    // and a cursor's first frame is the one its owner has already committed to.
    for (let index = 0; index < frames.length; index += 1) request.onFrameQueued?.(index);
    // A group that is waiting for a place in the queue is still the caller's to take back, and it is
    // NOT in a queue yet: `#pending` is what makes that state visible to `withdraw`. Without it, a
    // task whose waiter timed out while the queue was full could not be taken back at all, and its
    // frame would be written later with nobody left to hear about it.
    this.#admit(group);
    return {
      abandon: (): boolean => this.#abandon(group),
      done,
      withdraw: (): boolean => this.#withdraw(group),
    };
  }

  close(reason = new Error('Aura IPC frame scheduler closed')): void {
    if (this.#closed) return;
    this.#closed = true;
    this.#rejectEverythingHeld(reason);
  }

  /** Counts and byte totals only: no frame content and no request identity. */
  facts(): FrameSchedulerFacts {
    const committedBytes =
      bucketTotal(this.#queued, 'bytes') +
      bucketTotal(this.#staged, 'bytes') +
      this.#inFlightBytes +
      this.#inFlightOwedBytes;
    const committedFrames =
      bucketTotal(this.#queued, 'frames') +
      bucketTotal(this.#staged, 'frames') +
      (this.#inFlightPriority === undefined ? 0 : 1 + this.#inFlightOwedFrames);
    return {
      bytesWritten: this.#bytesWritten,
      closed: this.#closed,
      committedBytes,
      committedFrames,
      heldBytes: this.#heldBytes(),
      heldFrames: this.#heldFrames(),
      inFlight: this.#pumping,
      inFlightBytes: this.#inFlightBytes,
      inFlightFrames: this.#inFlightPriority === undefined ? 0 : 1,
      nextSequence: this.#nextSequence.toString(),
      peakCommittedBytes: this.#peakCommittedBytes,
      peakHeldBytes: this.#peakHeldBytes,
      peakHeldFrames: this.#peakHeldFrames,
      peakPendingBytes: this.#peakPendingBytes,
      peakPendingGroups: this.#peakPendingGroups,
      peakQueuedBytes: this.#peakQueuedBytes,
      peakQueuedFrames: this.#peakQueuedFrames,
      peakQueuedGroups: this.#peakQueuedGroups,
      peakStagedBytes: this.#peakStagedBytes,
      pendingBytes: bucketTotal(this.#pending, 'bytes'),
      pendingFrames: bucketTotal(this.#pending, 'frames'),
      pendingGroups: bucketTotal(this.#pending, 'groups'),
      queuedBytes: bucketTotal(this.#queued, 'bytes'),
      queuedFrames: bucketTotal(this.#queued, 'frames'),
      queuedGroups: bucketTotal(this.#queued, 'groups'),
      stagedBytes: bucketTotal(this.#staged, 'bytes'),
      stagedFrames: bucketTotal(this.#staged, 'frames'),
      stagedGroups: bucketTotal(this.#staged, 'groups'),
      withdrawnGroups: this.#withdrawnGroups,
      writesFailed: this.#writesFailed,
      writesStarted: this.#writesStarted,
      writesSucceeded: this.#writesSucceeded,
    };
  }

  /**
   * Register one frame group and let the scheduler decide when it can be queued.
   *
   * A group that can be queued now is queued now. One that cannot waits in its class's pending list - a
   * plain list, not a `for(;;)` loop around a promise - so a task that never gets space holds exactly
   * one entry and no closure chain.
   */
  #admit(group: ScheduledGroup): void {
    if (this.#closed) {
      group.reject(new FrameQueueClosedError('Aura IPC frame scheduler is closed'));
      return;
    }
    if (this.#canStartNow(group)) {
      this.#place(group, this.#queued);
      this.#queues[group.priority].push(group);
      void this.#pump();
      return;
    }
    this.#place(group, this.#pending);
    this.#pendingGroups[group.priority].push(group);
    void this.#pump();
  }

  /**
   * Offer space to the tasks waiting for it, highest class first, and report whether anything moved.
   *
   * Class order matters here and not only at selection: a P0 cancellation and an ordinary request can
   * both be waiting when a slot frees up, and admitting them in arrival order would let the ordinary one
   * spend the slot the reserved class exists for. A group leaves its pending list the moment it is
   * queued, so a task that is admitted once can never be admitted twice.
   */
  #drainPending(): boolean {
    let moved = false;
    for (
      let priority = 0 as QueuePriority;
      priority <= 3;
      priority = (priority + 1) as QueuePriority
    ) {
      const pending = this.#pendingGroups[priority];
      for (;;) {
        const group = pending[0];
        if (group === undefined) break;
        if (group.withdrawn) {
          pending.shift();
          this.#unplace(group, this.#pending);
          this.#withdrawnGroups += 1;
          group.reject(
            new FrameWithdrawnError('Aura IPC frame was withdrawn before it was written'),
          );
          moved = true;
          continue;
        }
        if (!this.#canFitCapacity(group, 'pending')) break;
        pending.shift();
        this.#unplace(group, this.#pending);
        this.#place(group, this.#queued);
        this.#queues[priority].push(group);
        moved = true;
      }
    }
    if (moved) this.#publishFacts();
    return moved;
  }

  /**
   * Take a group back. Only a group that has not been written yet can be withdrawn; once bytes have
   * reached the sink the caller owns an outcome it cannot pretend never happened.
   */
  #withdraw(group: ScheduledGroup): boolean {
    if (group.withdrawn || group.frameIndex > 0) return false;
    return this.#stopFeeding(group);
  }

  /**
   * Stop a group that still holds UNBUILT frames, whatever it has already written.
   *
   * WHY IT IS NOT `#withdraw`: a stream cursor that has written chunk one and staged chunk two is not
   * "withdrawn before anything happened" - it really wrote bytes - but its staged frame is a
   * declaration over a source buffer its owner is about to give back. Leaving that declaration in
   * `#stagedGroups` until some later scheduling turn would keep the whole source alive while the
   * owner's own accounting already reported it released. This drops it at once.
   *
   * WHY THE GUARD IS ABOUT BUCKETS, NOT ABOUT `frameIndex`: a group that is INSIDE the sink holds no
   * bucket (it was unplaced before the write), and an atomic binding's owed frames are an obligation,
   * never something to drop. A group that is in no bucket therefore holds nothing droppable, and this
   * reports "nothing happened" for it - which is also what makes it safe to call on every exit path.
   */
  #abandon(group: ScheduledGroup): boolean {
    if (group.withdrawn) return false;
    const queued = this.#queues[group.priority].includes(group);
    const pending = this.#pendingGroups[group.priority].includes(group);
    const staged = this.#stagedGroups.has(group);
    if (!queued && !pending && !staged) return false;
    return this.#stopFeeding(group);
  }

  /** The one place a group stops being fed: leave every bucket, drop its frames, reject its owner. */
  #stopFeeding(group: ScheduledGroup): boolean {
    group.withdrawn = true;
    this.#withdrawnGroups += 1;
    const queue = this.#queues[group.priority];
    const index = queue.indexOf(group);
    if (index >= 0) {
      queue.splice(index, 1);
      this.#unplace(group, this.#queued);
    }
    this.#unstage(group);
    const pending = this.#pendingGroups[group.priority];
    const pendingIndex = pending.indexOf(group);
    if (pendingIndex >= 0) {
      pending.splice(pendingIndex, 1);
      this.#unplace(group, this.#pending);
    }
    this.#releaseRenderedFrames(group);
    group.reject(new FrameWithdrawnError('Aura IPC frame was withdrawn before it was written'));
    this.#publishFacts();
    void this.#pump();
    return true;
  }

  /**
   * Whether a task may CLAIM its place in the queue right now.
   *
   * A frame that is queued behind a write already in progress is still the caller's to take back, and
   * taking it back is only possible while nothing of it has been built. Admitting it straight into the
   * queue while another frame is being written would make every frame unwithdrawable the instant it is
   * created, so a task that arrives while the wire is busy WAITS for its turn - and can therefore still
   * be taken back when its waiter gives up first. A P0 frame is exempt: it is the class reserved for
   * exactly this situation, and it has its own bounded queue.
   *
   * This is asked once, at admission. Once a task is waiting, `#canFitCapacity` decides when it moves -
   * asking this again at that point would be asking "is the wire busy", which is true precisely because
   * the frame this task is waiting behind is the one being written.
   */
  #canStartNow(group: ScheduledGroup): boolean {
    if (group.priority !== 0 && this.#pumping && bucketTotal(this.#queued, 'frames') === 0) {
      return false;
    }
    return this.#canFitCapacity(group, 'none');
  }

  /**
   * Whether one group fits its class's bounds, given where its own frames are already accounted for.
   *
   * `placed` names the bucket the group's frames currently occupy, so a group that is already
   * committed is not charged for itself twice. The bounds are on COMMITTED frames - queued, staged as
   * a cursor between chunks, and inside the sink - because those are the frames this scheduler has
   * promised to write.
   *
   * A group that is still WAITING for a place is deliberately not part of that sum, and this is not a
   * convenience: a waiting group is exactly what an admission decision is made about, so counting the
   * whole pending list against the same slots would make the bound self-blocking - once the pending
   * list alone reached the cap, no frame could ever be admitted from it again and the channel would
   * stop dead. What bounds the pending list is its OWNER, which reserves every byte a task can hold
   * before admitting it, so the declared total is bounded by the channel's total pending-byte bound.
   * `facts()` reports the pending bucket separately for exactly that reason.
   */
  #canFitCapacity(group: ScheduledGroup, placed: Placement): boolean {
    const frames = group.frames.length;
    const size = frameBytes(group.frames);
    const ownStagedFrames = placed === 'staged' ? frames : 0;
    const ownStagedBytes = placed === 'staged' ? size : 0;
    if (group.priority === 0) {
      return (
        this.#committedFrames([0], ownStagedFrames) + frames <= this.#limits.p0Frames &&
        this.#committedBytes([0], ownStagedBytes) + size <= this.#limits.p0Bytes
      );
    }
    if (
      this.#committedFrames(SHARED_PRIORITIES, ownStagedFrames) + frames >
        this.#limits.sharedFrames ||
      this.#committedBytes(SHARED_PRIORITIES, ownStagedBytes) + size > this.#limits.sharedBytes
    ) {
      return false;
    }
    if (group.priority === 1) return true;
    return (
      this.#committedFrames(BULK_PRIORITIES, ownStagedFrames) + frames <= this.#limits.bulkFrames &&
      this.#committedBytes(BULK_PRIORITIES, ownStagedBytes) + size <= this.#limits.bulkBytes
    );
  }

  #next(): ScheduledGroup | undefined {
    for (;;) {
      const critical = this.#queues[0].shift();
      if (critical !== undefined) return critical;
      let shared: ScheduledGroup | undefined;
      for (let index = 0; index < SHARED_SCHEDULE.length; index += 1) {
        const priority = SHARED_SCHEDULE[this.#scheduleCursor];
        this.#scheduleCursor = (this.#scheduleCursor + 1) % SHARED_SCHEDULE.length;
        if (priority === undefined) continue;
        shared = this.#queues[priority].shift();
        if (shared !== undefined) break;
      }
      if (shared === undefined) return undefined;
      return shared;
    }
  }

  async #pump(): Promise<void> {
    if (this.#pumping) return;
    this.#pumping = true;
    try {
      for (;;) {
        // Space that a queued frame has just released goes to the waiting tasks, highest class first.
        // Doing it here - at the boundary between two complete frames - is what turns "a control frame
        // may be scheduled" into "a control frame IS the next frame chosen".
        this.#drainPending();
        // A stream cursor whose previous chunk has been written comes back for its next chunk only
        // NOW, after the waiting tasks were offered the slot, and only when there is room for it.
        this.#requeueStaged();
        const group = this.#next();
        if (group === undefined) {
          // Nothing queued. Either a task is still waiting for space that has just been released, or a
          // cursor still owes the next chunk of its own sequence. Both are bounded: `#drainPending`
          // only admits groups that really fit, and `#stagedGroups` only ever holds cursors that wrote
          // a chunk and have another one to build.
          const drained = this.#drainPending();
          const requeued = this.#requeueStaged();
          if (!drained && !requeued) return;
          continue;
        }
        this.#unplace(group, this.#queued);
        if (group.withdrawn) {
          // Counted here as well as in `#withdraw`, so the number always means "frames really taken
          // back before anything of them was built", whichever route reached that state.
          this.#withdrawnGroups += 1;
          group.reject(
            new FrameWithdrawnError('Aura IPC frame was withdrawn before it was written'),
          );
          continue;
        }
        const outcome = await this.#writeGroup(group);
        if (outcome.kind === 'failed') {
          const { reason } = outcome;
          group.reject(reason);
          this.#failureCause = reason;
          this.#writesFailed += 1;
          this.#closed = true;
          this.#rejectEverythingHeld(reason);
          this.#onSinkFailure?.(reason);
          return;
        }
        if (outcome.kind === 'refused') {
          // The frame's own payload is invalid, or its owner took it back at the boundary. Nothing of
          // it was written, so this is NOT a channel failure: the frame is refused locally and the
          // number it would have spent is not committed.
          if (outcome.reason instanceof FrameWithdrawnError) {
            // "Nobody wants this frame any more" IS a withdrawal, however it was reached: counting it
            // as one keeps the withdrawal count describing frames really taken back, not the route
            // they happened to take.
            this.#withdrawnGroups += 1;
            group.reject(outcome.reason);
          } else {
            group.reject(new FrameBuildError(outcome.reason));
          }
          continue;
        }
        if (outcome.kind === 'staged') {
          // A stream cursor wrote one complete chunk and still owes the next one; it is re-queued at
          // the next boundary, so a task waiting for space is offered the slot first.
          continue;
        }
        group.resolve();
        this.#writesSucceeded += 1;
      }
    } finally {
      this.#pumping = false;
      this.#inFlightBytes = 0;
      this.#inFlightOwedBytes = 0;
      this.#inFlightOwedFrames = 0;
      this.#inFlightPriority = undefined;
      this.#publishFacts();
    }
  }

  /** Re-queue the cursor frames that are between two of their own chunks. */
  #requeueStaged(): boolean {
    let moved = false;
    for (const group of [...this.#stagedGroups]) {
      if (group.frames.length === 0) {
        this.#unstage(group);
        moved = true;
        continue;
      }
      if (!this.#canFitCapacity(group, 'staged')) continue;
      this.#unstage(group);
      this.#place(group, this.#queued);
      this.#queues[group.priority].push(group);
      moved = true;
    }
    if (moved) this.#publishFacts();
    return moved;
  }

  /** Reject every frame this scheduler still holds, queued, staged or waiting for space. */
  #rejectEverythingHeld(reason: Error): void {
    for (const queue of this.#queues) {
      for (const group of queue.splice(0)) {
        this.#unplace(group, this.#queued);
        this.#releaseRenderedFrames(group);
        group.reject(reason);
      }
    }
    for (const pending of this.#pendingGroups) {
      for (const group of pending.splice(0)) {
        this.#unplace(group, this.#pending);
        this.#releaseRenderedFrames(group);
        group.reject(reason);
      }
    }
    for (const group of [...this.#stagedGroups]) {
      this.#unstage(group);
      this.#releaseRenderedFrames(group);
      group.reject(reason);
    }
    this.#publishFacts();
  }

  /**
   * Writes one group: an atomic group back to back, a stream cursor one chunk per scheduling turn.
   *
   * `refused` means nothing of the frame was built (or the group was taken back at the boundary);
   * `failed` means bytes may already be on the wire. The two are never collapsed.
   */
  async #writeGroup(group: ScheduledGroup): Promise<WriteOutcome> {
    this.#inFlightPriority = group.priority;
    try {
      for (;;) {
        const frame = group.frames[0];
        if (frame === undefined) return { kind: 'written' };
        const first = group.frameIndex === 0;
        // Asked BEFORE anything is built or numbered. A frame nobody wants any more is exactly as
        // unsent as one whose payload is invalid, so it is refused the same way: no sequence number, no
        // bytes, and no channel failure.
        //
        // An atomic group is asked ONLY about its first frame: once that frame is committed the Core
        // may already hold half of the operation, so the frames after it are an obligation the group
        // took on, never a frame that can be half-withdrawn.
        const wanted = first ? group.wanted : group.wantedContinuation;
        if (this.#closed || wanted?.() === false) {
          // Nothing of a group that never wrote a frame reached the sink; a group that already wrote
          // one may hold half of itself, and saying "refused" about it would be a `not_sent` claim
          // about bytes that really went out.
          return group.frameIndex === 0
            ? {
                kind: 'refused',
                reason: this.#closed
                  ? new FrameQueueClosedError('Aura IPC frame scheduler is closed')
                  : new FrameWithdrawnError('Aura IPC frame was withdrawn before it was written'),
              }
            : {
                kind: 'failed',
                reason: new FrameWriteAbortedError(
                  'the channel was retired between two frames of an atomic binding',
                ),
              };
        }
        let bytes: Buffer;
        try {
          bytes = frame.build();
        } catch (error) {
          return this.#refuse(group, error instanceof Error ? error : new Error(String(error)));
        }
        if (bytes.byteLength !== frame.sizeBytes) {
          bytes.fill(0);
          return this.#refuse(
            group,
            new Error(
              `frame builder declared ${frame.sizeBytes} bytes but produced ${bytes.byteLength}`,
            ),
          );
        }
        // Committed here and nowhere else: this is the scheduling boundary, and the frame is about to
        // be the next bytes on the wire. A withdrawn or refused frame never reaches this line.
        this.#nextSequence += 1n;
        this.#writesStarted += 1;
        group.frames.shift();
        this.#inFlightBytes = bytes.byteLength;
        this.#inFlightOwedBytes = frameBytes(group.frames);
        this.#inFlightOwedFrames = group.frames.length;
        group.onFrameWriteStarted?.(group.frameIndex, frame.sizeBytes);
        try {
          await this.#sink.write(bytes);
        } catch (error) {
          const reason = error instanceof Error ? error : new Error(String(error));
          return { kind: 'failed', reason: new FrameWriteAbortedError(reason.message) };
        } finally {
          this.#inFlightBytes = 0;
          this.#inFlightOwedBytes = 0;
          this.#inFlightOwedFrames = 0;
          group.onFrameWriteSettled?.(group.frameIndex, bytes.byteLength);
          frame.release?.(bytes);
        }
        this.#bytesWritten += bytes.byteLength;
        group.frameIndex += 1;
        if (group.kind === 'atomic') {
          // The turn stays with this group: its remaining frames are written back to back, so nothing
          // can be scheduled between the command frame and the SecretInput frame.
          if (group.frames.length === 0) return { kind: 'written' };
          continue;
        }
        // A stream cursor exposes the NEXT chunk only at the next scheduling boundary, so a task
        // waiting for space - a P0 cancellation in particular - is offered the slot this chunk would
        // otherwise have taken. Returning HERE is what stops one stream from holding the wire.
        const next = group.next;
        if (next === undefined) return { kind: 'written' };
        let upcoming: ScheduledFrame | undefined;
        try {
          upcoming = next();
        } catch (error) {
          return this.#refuse(group, error instanceof Error ? error : new Error(String(error)));
        }
        // The cursor has no more chunks: the stream is complete, and every one of its frames was
        // written back to back with a real boundary in between.
        if (upcoming === undefined) return { kind: 'written' };
        group.frames.push(upcoming);
        this.#place(group, this.#staged);
        this.#stagedGroups.add(group);
        group.onFrameQueued?.(group.frameIndex);
        return { kind: 'staged' };
      }
    } finally {
      this.#inFlightPriority = undefined;
      this.#inFlightOwedBytes = 0;
      this.#inFlightOwedFrames = 0;
    }
  }

  /**
   * A frame that could not be built: nothing of it exists, so it never spends a sequence number.
   *
   * A group that already wrote a frame is a different fact - the bytes are on the wire - so its
   * ending is reported as a failure, never as a local refusal.
   */
  #refuse(
    group: ScheduledGroup,
    reason: Error,
  ): { kind: 'refused'; reason: Error } | { kind: 'failed'; reason: Error } {
    return group.frameIndex === 0
      ? { kind: 'refused', reason }
      : { kind: 'failed', reason: new FrameWriteAbortedError(reason.message) };
  }

  /** Move one group's frames into a bucket, so every bucket is a real byte total. */
  #place(group: ScheduledGroup, bucket: Occupancy[]): void {
    const entry = bucket[group.priority];
    if (entry === undefined) return;
    entry.bytes += frameBytes(group.frames);
    entry.frames += group.frames.length;
    entry.groups += 1;
    this.#publishFacts();
  }

  /** Take one group's frames out of a bucket. */
  #unplace(group: ScheduledGroup, bucket: Occupancy[]): void {
    const entry = bucket[group.priority];
    if (entry === undefined) return;
    entry.bytes -= frameBytes(group.frames);
    entry.frames -= group.frames.length;
    entry.groups -= 1;
    if (entry.bytes < 0) entry.bytes = 0;
    if (entry.frames < 0) entry.frames = 0;
    if (entry.groups < 0) entry.groups = 0;
    this.#publishFacts();
  }

  #unstage(group: ScheduledGroup): void {
    if (!this.#stagedGroups.delete(group)) return;
    this.#unplace(group, this.#staged);
  }

  /**
   * Frames this scheduler has COMMITTED to write, for a set of classes.
   *
   * Committed means: holding a place in a queue, held as a cursor frame between two chunks, or
   * inside the sink. A frame that is only waiting for a place in a queue is deliberately NOT counted
   * here - see `#canFitCapacity`.
   */
  #committedFrames(priorities: readonly QueuePriority[], ownStagedFrames: number): number {
    return (
      sumIn(this.#queued, priorities, 'frames') +
      sumIn(this.#staged, priorities, 'frames') -
      ownStagedFrames +
      (this.#inFlightPriority !== undefined && priorities.includes(this.#inFlightPriority)
        ? 1 + this.#inFlightOwedFrames
        : 0)
    );
  }

  /** Bytes this scheduler has COMMITTED to write, for a set of classes. Same rule as the frames. */
  #committedBytes(priorities: readonly QueuePriority[], ownStagedBytes: number): number {
    return (
      sumIn(this.#queued, priorities, 'bytes') +
      sumIn(this.#staged, priorities, 'bytes') -
      ownStagedBytes +
      (this.#inFlightPriority !== undefined && priorities.includes(this.#inFlightPriority)
        ? this.#inFlightBytes + this.#inFlightOwedBytes
        : 0)
    );
  }

  /** Every byte this scheduler has been asked to keep alive, in one number. */
  #heldBytes(): number {
    return (
      bucketTotal(this.#pending, 'bytes') +
      bucketTotal(this.#queued, 'bytes') +
      bucketTotal(this.#staged, 'bytes') +
      this.#inFlightBytes +
      this.#inFlightOwedBytes
    );
  }

  /** Every frame this scheduler has been asked to keep alive, in one number. */
  #heldFrames(): number {
    return (
      bucketTotal(this.#pending, 'frames') +
      bucketTotal(this.#queued, 'frames') +
      bucketTotal(this.#staged, 'frames') +
      (this.#inFlightPriority === undefined ? 0 : 1 + this.#inFlightOwedFrames)
    );
  }

  /**
   * Drop every reference this scheduler still holds to frames that will never be written.
   *
   * Only the OWNER of a frame's bytes knows whether they are secret, so the scheduler never guesses:
   * it drops its references and whoever built the frame keeps its own release path. This must only run
   * on a group that is NOT about to be written - the frames of a selected group still have to be built
   * at the scheduling boundary.
   */
  #releaseRenderedFrames(group: ScheduledGroup): void {
    group.frames.length = 0;
  }

  #publishFacts(): void {
    const pendingBytes = bucketTotal(this.#pending, 'bytes');
    const pendingGroups = bucketTotal(this.#pending, 'groups');
    const queuedBytes = bucketTotal(this.#queued, 'bytes');
    const queuedFrames = bucketTotal(this.#queued, 'frames');
    const queuedGroups = bucketTotal(this.#queued, 'groups');
    const stagedBytes = bucketTotal(this.#staged, 'bytes');
    const heldBytes = this.#heldBytes();
    const heldFrames = this.#heldFrames();
    const committedBytes =
      queuedBytes + stagedBytes + this.#inFlightBytes + this.#inFlightOwedBytes;
    if (pendingBytes > this.#peakPendingBytes) this.#peakPendingBytes = pendingBytes;
    if (pendingGroups > this.#peakPendingGroups) this.#peakPendingGroups = pendingGroups;
    if (queuedBytes > this.#peakQueuedBytes) this.#peakQueuedBytes = queuedBytes;
    if (queuedFrames > this.#peakQueuedFrames) this.#peakQueuedFrames = queuedFrames;
    if (queuedGroups > this.#peakQueuedGroups) this.#peakQueuedGroups = queuedGroups;
    if (stagedBytes > this.#peakStagedBytes) this.#peakStagedBytes = stagedBytes;
    if (heldBytes > this.#peakHeldBytes) this.#peakHeldBytes = heldBytes;
    if (heldFrames > this.#peakHeldFrames) this.#peakHeldFrames = heldFrames;
    if (committedBytes > this.#peakCommittedBytes) this.#peakCommittedBytes = committedBytes;
  }
}
