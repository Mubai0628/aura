/**
 * Ownership of everything this Main process sends over ONE authenticated Core channel.
 *
 * WHY THIS MODULE EXISTS. The transport used to serialise a WHOLE logical send - every chunk of a
 * bulk stream included - behind one physical write promise (`#serializeOutbound`), and `enqueue` took
 * the finished bytes. Two consequences were reproduced against the production transport with a
 * controlled sink:
 *
 *  1. One ordinary frame whose physical write never finished had no owner with a budget of its own.
 *     The caller's request budget expired, the request was reported as unknown, and the connection
 *     was never retired: a frame that may have written half of itself stayed on a channel nobody
 *     could prove anything about.
 *  2. Because the whole stream held the lock, a P0 control frame could not be considered until the
 *     entire bulk stream had been written, so the observed order was bulk/bulk/control.
 *
 * WHAT THIS MODULE OWNS, and what it deliberately does not:
 *
 *  - FRAME PREPARATION and the OUTBOUND SEQUENCE. A frame is built at the scheduling boundary inside
 *    `FrameScheduler`, so the number it carries is committed in true send order, and a frame that is
 *    withdrawn, refused by its own validation or lost with the channel never spends a number.
 *  - THREE SEPARATE DEADLINES, because they measure three different things and are not
 *    interchangeable (see below).
 *  - The ATOMIC SecretInput BINDING: the command frame and the SecretInput frame are ONE atomic
 *    group written back to back, so no other frame can be scheduled between them. Once the command
 *    frame has been committed the binding is an obligation: it is never half-withdrawn.
 *  - The BULK STREAM as a CURSOR. A stream is NOT one indivisible frame and is NOT capped at two
 *    chunks: it exposes one complete chunk frame at a time, which is what gives a P0 control frame
 *    its legitimate scheduling boundary between two complete chunks.
 *  - TWO byte budgets with DIFFERENT SCOPES, because the bytes they measure are held by different
 *    owners for different lengths of time. The SOURCE budget covers what this module keeps a
 *    reference to for a task's whole life (a request's serialised payload, a bulk stream's whole
 *    source buffer, the send-owned SecretInput copy); the FRAME WRITE WORKING budget covers the one
 *    encoded frame the scheduler has built and handed to the sink PLUS the copy the native addon
 *    makes of those bytes for the duration of the real write promise. A whole-stream source size is
 *    therefore never merged into a per-chunk working allowance, and the whole-channel figure this
 *    owner advertises is the SUM of the two - a statement about the bytes this send path holds, and
 *    deliberately not a claim about whole-process RSS (the caller that read a file into a Buffer
 *    still holds its own reference to the same bytes).
 *  - It does NOT own settlement. A timeout, a rejection, a cancellation, a writer error and a close
 *    all end in the transport's ONE settlement path; this module reports what it knows (was anything
 *    written? did the build fail before that?) and hands the decision back.
 *
 * THE THREE DEADLINES, and why one number cannot serve all three:
 *
 *  - QUEUE deadline: how long ONE frame may wait for the wire before it is dropped, provably unsent.
 *    It ends when that frame is really handed to the sink.
 *  - FRAME WRITE deadline: how long ONE COMPLETE FRAME may take between being handed to the sink and
 *    its write promise settling. A new frame starts a new one, so a stream that keeps finishing
 *    frames is never mistaken for a single hung write and never measures the total serialised time
 *    of everything before it. The native API reports no per-byte progress, so this is a deadline on
 *    a complete-frame write and nothing more is claimed about it.
 *  - ATOMIC GROUP deadline: how long the WHOLE "command + SecretInput" binding may take, counted from
 *    the moment its command frame is really dispatched. It is deliberately NOT restarted when the
 *    second frame begins: after the command frame is committed the Core may already hold half the
 *    operation, and starting frame two does not un-commit frame one.
 */
import {
  type AtomicFrameGroupRequest,
  FrameBuildError,
  FrameQueueClosedError,
  type FrameScheduler,
  type FrameStreamRequest,
  FrameTooLargeError,
  FrameWithdrawnError,
  p0FrameIsOversized,
  type ScheduledFrame,
} from './frame-scheduler.js';
import {
  encodeBinaryFrame,
  encodeControlFrame,
  encodeSecretInputFrame,
  type IpcFrameBinding,
  MAX_BINARY_CHUNK_BYTES,
  validateSecretInputBytes,
} from './protocol.js';
import {
  MAX_FRAME_WORKING_BYTES,
  MAX_RETAINED_SEND_BYTES,
  MAX_RETAINED_SOURCE_BYTES,
  PHYSICAL_SEND_BUDGET_MS,
  SECRET_INPUT_BINDING_BUDGET_MS,
  SendBudgetExceededError,
  binaryFrameSize,
  controlFrameSize,
  secretInputFrameSize,
  sizedFrame,
} from './outbound-send/budgets.js';
import {
  type OutboundRequestSpec,
  type OutboundSendOwnerFacts,
  type OutboundSendOwnerHandlers,
  type OutboundTask,
  type OutboundTaskKey,
  type OutboundTaskOutcome,
  type OutboundTaskState,
  SOURCE_LIMITS,
  SecretInputOwnership,
  type SourceClass,
  type TaskRecord,
  asError,
} from './outbound-send/tasks.js';
export {
  controlFrameSize,
  secretInputFrameSize,
  binaryFrameSize,
  PHYSICAL_SEND_BUDGET_MS,
  SECRET_INPUT_BINDING_BUDGET_MS,
  MAX_TOOL_DRAFT_FILE_BYTES,
  P0_SOURCE_BUDGET_BYTES,
  CONTROL_SOURCE_BUDGET_BYTES,
  STREAM_SOURCE_BUDGET_BYTES,
  MAX_RETAINED_SOURCE_BYTES,
  MAX_ENCODED_FRAME_BYTES,
  MAX_FRAME_WORKING_BYTES,
  MAX_RETAINED_SEND_BYTES,
  SendBudgetExceededError,
} from './outbound-send/budgets.js';
export { SecretInputOwnership, createTaskKey } from './outbound-send/tasks.js';
export type {
  OutboundTaskOutcome,
  OutboundTaskState,
  OutboundRequestSpec,
  OutboundSendOwnerHandlers,
  OutboundTaskEnding,
  OutboundTaskKey,
  OutboundTask,
  OutboundSendOwnerFacts,
} from './outbound-send/tasks.js';

/**
 * The owner of this channel's physical sends.
 *
 * It knows the channel's frame binding and its scheduler; it does NOT know the connection, the
 * registry or the caller. Everything it cannot decide alone is reported through its handlers, which
 * is what keeps the connection's single retirement path in one place.
 */
export class OutboundSendOwner {
  readonly #binding: IpcFrameBinding;
  readonly #handlers: OutboundSendOwnerHandlers;
  readonly #physicalSendBudgetMs: number;
  readonly #records = new Set<TaskRecord>();
  readonly #scheduler: FrameScheduler;
  readonly #secretInputBudgetMs: number;
  /** SOURCE bytes retained by each class. Their sum is the channel's source figure. */
  #sourceControlBytes = 0;
  #sourceP0Bytes = 0;
  #sourceStreamBytes = 0;
  /**
   * The encoded frame inside the sink and the copy the native addon makes of it, for the ONE write that
   * is outstanding. Both are given back by that write's own promise, never by a task settling.
   */
  #frameInFlightBytes = 0;
  #nativeCopyBytes = 0;
  #peakFrameBytes = 0;
  #peakNativeBytes = 0;
  #peakPendingBytes = 0;
  #peakSourceBytes = 0;
  #physicallyStuck = 0;
  #refusedBeforeWrite = 0;
  #refusalsOverBudget = 0;
  #tasks = 0;
  #withdrewBeforeWrite = 0;
  #wroteAnything = 0;

  constructor(
    binding: IpcFrameBinding,
    scheduler: FrameScheduler,
    handlers: OutboundSendOwnerHandlers,
    limits: {
      physicalSendBudgetMs?: number;
      secretInputBindingBudgetMs?: number;
    } = {},
  ) {
    this.#binding = binding;
    this.#handlers = handlers;
    this.#physicalSendBudgetMs = limits.physicalSendBudgetMs ?? PHYSICAL_SEND_BUDGET_MS;
    this.#scheduler = scheduler;
    this.#secretInputBudgetMs = limits.secretInputBindingBudgetMs ?? SECRET_INPUT_BINDING_BUDGET_MS;
  }

  /** Counts only: how many tasks this owner ran, how they ended, and what it is holding. */
  facts(): OutboundSendOwnerFacts {
    return {
      controlSourceBytes: this.#sourceControlBytes,
      frameBytes: this.#frameInFlightBytes,
      frameWriteBudgetMs: this.#physicalSendBudgetMs,
      maxFrameWorkingBytes: MAX_FRAME_WORKING_BYTES,
      maxPendingBytes: MAX_RETAINED_SEND_BYTES,
      maxSourceBytes: MAX_RETAINED_SOURCE_BYTES,
      nativeBytes: this.#nativeCopyBytes,
      p0SourceBytes: this.#sourceP0Bytes,
      peakFrameBytes: this.#peakFrameBytes,
      peakNativeBytes: this.#peakNativeBytes,
      peakPendingBytes: this.#peakPendingBytes,
      peakSourceBytes: this.#peakSourceBytes,
      pendingBytes: this.#retainedBytes(),
      physicalSendBudgetMs: this.#physicalSendBudgetMs,
      physicallyStuck: this.#physicallyStuck,
      queueWaitBudgetMs: this.#physicalSendBudgetMs,
      refusalsOverBudget: this.#refusalsOverBudget,
      refusedBeforeWrite: this.#refusedBeforeWrite,
      secretInputBindingBudgetMs: this.#secretInputBudgetMs,
      sourceBytes: this.#sourceBytes(),
      streamSourceBytes: this.#sourceStreamBytes,
      tasks: this.#tasks,
      watching: this.#records.size,
      withdrewBeforeWrite: this.#withdrewBeforeWrite,
      wroteAnything: this.#wroteAnything,
    };
  }

  /** How many tasks still hold something. A leak shows up here as a number that never returns to 0. */
  get watching(): number {
    return this.#records.size;
  }

  /** Every SOURCE byte this owner retains for a live task, across all three classes. */
  #sourceBytes(): number {
    return this.#sourceP0Bytes + this.#sourceControlBytes + this.#sourceStreamBytes;
  }

  /**
   * Every byte this owner can be shown to hold right now: the retained sources, the encoded frame that
   * is inside the sink, and the native copy of that frame. The three are added because they are three
   * different holders of three different byte ranges, not because one number bounds them.
   */
  #retainedBytes(): number {
    return this.#sourceBytes() + this.#frameInFlightBytes + this.#nativeCopyBytes;
  }

  /** Republish the peaks after any change to what this owner holds. */
  #touchPeaks(): void {
    const source = this.#sourceBytes();
    const retained = this.#retainedBytes();
    if (source > this.#peakSourceBytes) this.#peakSourceBytes = source;
    if (this.#frameInFlightBytes > this.#peakFrameBytes)
      this.#peakFrameBytes = this.#frameInFlightBytes;
    if (this.#nativeCopyBytes > this.#peakNativeBytes)
      this.#peakNativeBytes = this.#nativeCopyBytes;
    if (retained > this.#peakPendingBytes) this.#peakPendingBytes = retained;
  }

  /**
   * Register one ordinary request as its own send task.
   *
   * Throws only for a refusal that is PROVABLY unsent - an oversized P0 frame, an invalid secret, a
   * missing invocation id, or a task whose SOURCE does not fit its class's budget - so the transport
   * can report `not_sent` without registering anything.
   */
  register(spec: OutboundRequestSpec, key: OutboundTaskKey): OutboundTask {
    const controlBytes = controlFrameSize(spec.framePayload.byteLength);
    if (spec.priority === 0 && p0FrameIsOversized(controlBytes)) {
      this.#refusedBeforeWrite += 1;
      throw new FrameTooLargeError('P0 frame exceeds its 32 KiB emergency frame limit');
    }
    const secretBytes = spec.secret?.ownership.takeOver();
    const operation = spec.secret?.operation;
    const isBinding = secretBytes !== undefined && operation !== undefined;
    if (isBinding && spec.commandInvocationId === undefined) {
      spec.secret?.ownership.release();
      this.#refusedBeforeWrite += 1;
      throw new Error('SecretInput requires a CommandInvocationId');
    }
    if (secretBytes !== undefined) {
      // Validated BEFORE the command frame is committed, so an invalid secret can never leave the
      // Core holding half of an atomic credential operation - and never spends a sequence number.
      try {
        validateSecretInputBytes(secretBytes);
      } catch (error) {
        spec.secret?.ownership.release();
        this.#refusedBeforeWrite += 1;
        throw error;
      }
    }
    const secretFrameBytes =
      isBinding && secretBytes !== undefined ? secretInputFrameSize(secretBytes.byteLength) : 0;
    // The SOURCE reservation is the payload plus the send-owned secret copy: the two buffers this owner
    // keeps for the task's whole life. The frames the payload can become are working bytes, bounded by
    // `MAX_FRAME_WORKING_BYTES` at the moment each one is really inside the sink.
    const reservedBytes = spec.framePayload.byteLength + (secretBytes?.byteLength ?? 0);
    const sourceClass: SourceClass = spec.priority === 0 ? 'p0' : 'control';
    const record = this.#record(spec, key, sourceClass, spec.secret?.ownership, isBinding ? 2 : 0);
    try {
      this.#reserveSource(record, reservedBytes);
    } catch (error) {
      this.#finish(record, 'refused', asError(error), false);
      throw error;
    }
    const frames: ScheduledFrame[] = [
      sizedFrame(controlBytes, () =>
        encodeControlFrame(spec.framePayload, this.#binding, this.#scheduler.nextSequence),
      ),
    ];
    if (isBinding && secretBytes !== undefined) {
      // The TWO frames of the atomic binding are declared together and written back to back, with
      // nothing scheduled between them. There is no continuation predicate: after the command frame is
      // committed the binding is owed, and the frame's own encoder zeroes the secret as it copies it.
      frames.push(
        sizedFrame(
          secretFrameBytes,
          () =>
            encodeSecretInputFrame({
              binding: this.#binding,
              commandInvocationId: spec.commandInvocationId as string,
              correlationId: spec.correlationId,
              operation,
              secret: secretBytes,
              sequence: this.#scheduler.nextSequence,
            }),
          (bytes) => bytes.fill(0),
        ),
      );
    }
    // The handle exists BEFORE the frame is queued on purpose: a task may settle synchronously the
    // moment it is admitted (a refusal, or a frame that is simply written immediately), and whoever
    // registered it must already be able to recognise which task ended.
    const task = this.#taskOf(record);
    // The task's own predicate, when it has one, is the STARTING answer; the scheduler is handed a
    // closure that reads it LIVE, so a `setWanted` that arrives after registration still decides the
    // frame at the moment the scheduler would really build it.
    if (spec.wanted !== undefined) record.wanted = spec.wanted;
    this.#enqueue(record, {
      frames,
      kind: 'atomic',
      label: record.key.label,
      onFrameQueued: () => this.#armQueueDeadline(record),
      onFrameWriteSettled: (_index, sizeBytes) => this.#frameSettled(record, sizeBytes),
      onFrameWriteStarted: (index, sizeBytes) => this.#frameStarted(record, index, sizeBytes),
      priority: record.priority,
      wanted: () => this.#wantedNow(record, false),
    });
    return task;
  }

  /**
   * Send one bulk stream as a cursor over COMPLETE frames.
   *
   * This is the second reproduced deviation: the stream used to hold the whole outbound lock, so a P0
   * control frame had no legal boundary to use, and passing its total chunk count into the atomic
   * group's frame limit made any stream longer than two chunks fail before a single byte was written.
   * Here the cursor exposes ONE chunk at a time and the next chunk is only built once the previous one
   * has really settled - so a control frame queued in between is simply the next frame chosen, the
   * chunk count is not bounded by the atomic ceiling, and no frame is ever interrupted mid-write.
   */
  async sendBinaryStream(streamId: string, bytes: Buffer, key: OutboundTaskKey): Promise<void> {
    if (bytes.byteLength === 0) {
      throw new Error('a bulk stream cannot carry zero bytes');
    }
    const record = this.#record({ taskLabel: key.label }, key, 'stream', undefined, 0);
    // A stream has no registry record of its own, so its OWN lifetime answers "is this still wanted".
    // The first chunk is the commitment and may already be mid-write when a caller stops waiting; the
    // chunks that follow it are gated, so an abandoned stream stops feeding a wire nobody reads.
    record.wanted = () => true;
    record.wantedContinuation = () => !record.doneResolved;
    // The reservation covers the whole SOURCE buffer and nothing else: the file this module keeps a
    // reference to for the whole stream. The chunk frame the cursor can hold, and the copy the native
    // addon makes of it while it is really being written, are working bytes - bounded by
    // `MAX_FRAME_WORKING_BYTES` at the moment each chunk is inside the sink, never merged into this
    // whole-stream figure. A file the business layer accepts is therefore admitted here whatever its
    // length, up to one maximum Tool Draft file.
    const reservedBytes = bytes.byteLength;
    try {
      this.#reserveSource(record, reservedBytes);
    } catch (error) {
      this.#finish(record, 'refused', asError(error), false);
      throw error;
    }
    let chunkSequence = 0n;
    let offset = 0;
    const nextFrame = (): ScheduledFrame | undefined => {
      if (offset >= bytes.byteLength) return undefined;
      const start = offset;
      const end = Math.min(start + MAX_BINARY_CHUNK_BYTES, bytes.byteLength);
      offset = end;
      const sequence = chunkSequence;
      chunkSequence += 1n;
      const data = bytes.subarray(start, end);
      return sizedFrame(binaryFrameSize(data.byteLength), () =>
        encodeBinaryFrame({
          binding: this.#binding,
          chunkSequence: sequence,
          data,
          endOfMessage: end === bytes.byteLength,
          sequence: this.#scheduler.nextSequence,
          streamId,
        }),
      );
    };
    const first = nextFrame();
    if (first === undefined) throw new Error('a bulk stream cannot carry zero bytes');
    const task = this.#taskOf(record);
    this.#enqueue(record, {
      first,
      kind: 'stream',
      label: record.key.label,
      // The first chunk is always admitted: it IS the commitment, and it is often already in the sink
      // when the caller stops waiting. Only the chunks that follow it can still be stopped.
      next: nextFrame,
      onFrameQueued: () => this.#armQueueDeadline(record),
      onFrameWriteSettled: (_index, sizeBytes) => this.#frameSettled(record, sizeBytes),
      onFrameWriteStarted: (index, sizeBytes) => this.#frameStarted(record, index, sizeBytes),
      priority: record.priority,
      wanted: () => this.#wantedNow(record, false),
      wantedContinuation: () => this.#wantedNow(record, true),
    });
    void task;
    await record.done;
    // The stream's failure is reported to ITS caller: `sendBinaryStream` is awaited by a supervisor
    // that has to know the bytes did not all go out, and the channel's own retirement was already
    // decided by the send owner when the task ended. A stream that did not end `written` did NOT send
    // every chunk, so it can never resolve silently.
    const failure = record.failure;
    if (failure !== undefined) throw failure;
    if (record.state !== 'written') {
      throw new Error(`bulk stream ended as ${record.state} before every chunk was written`);
    }
  }

  /**
   * Reserve a task's SOURCE bytes BEFORE any of them exists.
   *
   * The check is against the task's own CLASS, so a saturated bulk class cannot refuse the reserved P0
   * class that exists to drain it, and a maximum Tool Draft file cannot refuse a maximum catalog frame.
   * A refusal here is a local, transient capacity fact: nothing was built and nothing was numbered.
   */
  #reserveSource(record: TaskRecord, reservedBytes: number): void {
    const limitBytes = SOURCE_LIMITS[record.sourceClass];
    const held = this.#sourceHeld(record.sourceClass);
    if (held + reservedBytes > limitBytes) {
      this.#refusalsOverBudget += 1;
      throw new SendBudgetExceededError(record.sourceClass, held, reservedBytes, limitBytes);
    }
    this.#addSource(record.sourceClass, reservedBytes);
    record.reservedBytes = reservedBytes;
    this.#touchPeaks();
  }

  /** The source bytes one class holds right now. */
  #sourceHeld(sourceClass: SourceClass): number {
    if (sourceClass === 'p0') return this.#sourceP0Bytes;
    return sourceClass === 'stream' ? this.#sourceStreamBytes : this.#sourceControlBytes;
  }

  /** Add to one class's source total. The sum of the three is the channel's source figure. */
  #addSource(sourceClass: SourceClass, bytes: number): void {
    if (sourceClass === 'p0') this.#sourceP0Bytes += bytes;
    else if (sourceClass === 'stream') this.#sourceStreamBytes += bytes;
    else this.#sourceControlBytes += bytes;
  }

  /** Give a task's SOURCE bytes back. Idempotent, so every exit path can call it. */
  #releaseSourceReservation(record: TaskRecord): void {
    if (record.reservedBytes === 0) return;
    this.#addSource(record.sourceClass, -record.reservedBytes);
    record.reservedBytes = 0;
    this.#touchPeaks();
  }

  /**
   * Take the working bytes of the frame that is about to be the next bytes on the wire.
   *
   * Called from the scheduler's own "this frame's write begins" hook, which runs in the SAME synchronous
   * block as the frame's build and immediately before the sink call. It therefore covers exactly the
   * two frame bodies that exist at once: the encoded JS frame the scheduler built, and the copy the
   * native addon makes of it inside `write` before returning its promise.
   *
   * WHY IT CANNOT FAIL: the budget is two maximum encoded frames (see `MAX_FRAME_WORKING_BYTES`) and the
   * sink is strictly serialised, so at most one frame - and therefore one copy - is ever outstanding.
   * A frame's own encoder bound is what keeps the two equal.
   */
  #takeFrameWorking(record: TaskRecord, sizeBytes: number): void {
    record.framesInFlight += 1;
    this.#frameInFlightBytes += sizeBytes;
    this.#nativeCopyBytes += sizeBytes;
    this.#touchPeaks();
  }

  /**
   * Give the working bytes back - on the REAL write promise, in the scheduler's settle hook.
   *
   * It is deliberately NOT reachable from a task settling. A caller that stops waiting, a queue
   * deadline, a physical deadline and a channel close all leave the native copy exactly where it is:
   * the addon still holds it, so admitting more bytes on the strength of a released reservation would
   * be a claim about memory that is still in use.
   */
  #settleFrameWorking(record: TaskRecord, sizeBytes: number): void {
    if (record.framesInFlight === 0) return;
    record.framesInFlight -= 1;
    this.#frameInFlightBytes = Math.max(0, this.#frameInFlightBytes - sizeBytes);
    this.#nativeCopyBytes = Math.max(0, this.#nativeCopyBytes - sizeBytes);
    this.#touchPeaks();
  }

  #record(
    spec: Partial<OutboundRequestSpec>,
    key: OutboundTaskKey,
    sourceClass: SourceClass,
    secretOwnership: SecretInputOwnership | undefined,
    atomicFrames: number,
  ): TaskRecord {
    let resolveDone: () => void = () => undefined;
    const done = new Promise<void>((resolve) => {
      resolveDone = resolve;
    });
    const record: TaskRecord = {
      abandonHeld: undefined,
      atomicFrames,
      done,
      doneResolved: false,
      failure: undefined,
      frameTimer: undefined,
      framesInFlight: 0,
      groupTimer: undefined,
      key,
      method: spec.method,
      physicallyStuck: false,
      priority: spec.priority ?? 1,
      queueTimer: undefined,
      reservedBytes: 0,
      schedulerSettled: true,
      resolveDone: () => resolveDone(),
      secretOwnership,
      sourceClass,
      state: 'registered',
      task: undefined,
      wanted: undefined,
      wantedContinuation: undefined,
      withdrawNow: undefined,
      writtenFrames: 0,
    };
    this.#tasks += 1;
    this.#records.add(record);
    return record;
  }

  /**
   * The ONE place a task's frames are handed to the scheduler.
   *
   * A synchronous throw from `enqueue` is a refusal of a task nothing of which exists, so it settles
   * the task HERE instead of escaping as an exception: a record that escaped would keep its byte
   * reservation, its timers and its place in `#records` for the process's lifetime, and the channel
   * would show a task that never ends.
   */
  #enqueue(record: TaskRecord, request: AtomicFrameGroupRequest | FrameStreamRequest): void {
    record.state = 'queued';
    record.schedulerSettled = false;
    let handle: {
      readonly abandon: () => boolean;
      readonly done: Promise<void>;
      readonly withdraw: () => boolean;
    };
    try {
      handle = this.#scheduler.enqueue(request);
    } catch (error) {
      record.schedulerSettled = true;
      this.#finish(record, 'refused', asError(error), false);
      return;
    }
    record.abandonHeld = handle.abandon;
    record.withdrawNow = handle.withdraw;
    handle.done.then(
      () => {
        this.#schedulerSettled(record);
        this.#finish(record, 'written', undefined, false);
      },
      (error: unknown) => {
        this.#schedulerSettled(record);
        const reason = asError(error);
        if (reason instanceof FrameBuildError || reason instanceof FrameTooLargeError) {
          // The frame's own payload is invalid AND nothing of it reached the sink. This is
          // request-local: the channel stays healthy and the caller hears `not_sent`.
          this.#finish(record, 'refused', reason, false);
          return;
        }
        if (reason instanceof FrameWithdrawnError || reason instanceof FrameQueueClosedError) {
          // Taken back before any frame was built, or the channel ended while this frame was queued:
          // either way nothing of this task reached the sink, so this task retires nothing.
          this.#finish(record, 'withdrawn', reason, false);
          return;
        }
        // A physical write failed, or the scheduler was closed by such a failure. The byte stream can
        // no longer be shown to be intact, so the CONNECTION owner retires it exactly once.
        this.#finish(record, 'failed', reason, true);
      },
    );
  }

  /** End the builder ownership even if the caller's task ended before the physical write. */
  #schedulerSettled(record: TaskRecord): void {
    record.schedulerSettled = true;
    // Handles close over the scheduler group and its stream cursor. Do not retain that source
    // through an already-settled task handle after returning its reservation.
    record.abandonHeld = undefined;
    record.withdrawNow = undefined;
    this.#releaseSourceReservation(record);
  }

  /**
   * Frame `index` of this task is about to be the next bytes on the wire.
   *
   * The queue wait is over for THIS frame; this write gets a deadline of its own, counted from here
   * rather than from the moment the task was registered; and for an atomic binding the whole-group
   * deadline starts with its command frame and is NOT restarted by the frame that follows it.
   *
   * It is also where the frame's WORKING bytes are taken: the scheduler calls this synchronously,
   * between building the frame and handing it to the sink, so the reservation exists before the native
   * copy does and covers the JS frame and that copy together.
   */
  #frameStarted(record: TaskRecord, index: number, sizeBytes: number): void {
    record.writtenFrames += 1;
    record.state = 'writing';
    this.#takeFrameWorking(record, sizeBytes);
    this.#clearQueueDeadline(record);
    this.#armFrameDeadline(record);
    if (index === 0 && record.atomicFrames > 0) this.#armGroupDeadline(record);
    this.#handlers.onWriteStarted(record.key);
  }

  /** ONE complete-frame write really settled, successfully or not: give its working bytes back. */
  #frameSettled(record: TaskRecord, sizeBytes: number): void {
    this.#clearFrameDeadline(record);
    this.#settleFrameWorking(record, sizeBytes);
  }

  /** How long the frame waiting right now may still wait for the wire. */
  #armQueueDeadline(record: TaskRecord): void {
    if (record.doneResolved) return;
    this.#clearQueueDeadline(record);
    record.queueTimer = setTimeout(() => {
      record.queueTimer = undefined;
      const reason = new Error(
        `outbound frame did not reach the sink within ${this.#physicalSendBudgetMs}ms`,
      );
      // Release the scheduler's frame builders before returning this task's reservation. A live
      // wanted predicate alone prevents dispatch later, but leaves the queued payload retained now.
      if (record.writtenFrames === 0) record.withdrawNow?.();
      this.#finish(record, 'withdrawn', reason, false);
    }, this.#physicalSendBudgetMs);
    record.queueTimer.unref?.();
  }

  /** How long the frame that is inside the sink right now may still take. */
  #armFrameDeadline(record: TaskRecord): void {
    this.#clearFrameDeadline(record);
    record.frameTimer = setTimeout(() => {
      record.frameTimer = undefined;
      record.physicallyStuck = true;
      this.#physicallyStuck += 1;
      const reason = new Error(
        `physical send did not progress within ${this.#physicalSendBudgetMs}ms`,
      );
      if (!this.#finish(record, 'failed', reason, true)) return;
      this.#handlers.onUnrecoverable(reason);
    }, this.#physicalSendBudgetMs);
    record.frameTimer.unref?.();
  }

  /**
   * How long the WHOLE atomic binding may still take, counted from its command frame's dispatch.
   *
   * It is deliberately armed once, for frame one, and never re-armed for frame two: the obligation was
   * taken on when the command frame was committed, and committing a second frame does not reset it.
   */
  #armGroupDeadline(record: TaskRecord): void {
    this.#clearGroupDeadline(record);
    record.groupTimer = setTimeout(() => {
      record.groupTimer = undefined;
      record.physicallyStuck = true;
      this.#physicallyStuck += 1;
      const reason = new Error(
        `the atomic SecretInput binding did not complete within ${this.#secretInputBudgetMs}ms`,
      );
      if (!this.#finish(record, 'failed', reason, true)) return;
      this.#handlers.onUnrecoverable(reason);
    }, this.#secretInputBudgetMs);
    record.groupTimer.unref?.();
  }

  #clearQueueDeadline(record: TaskRecord): void {
    if (record.queueTimer !== undefined) clearTimeout(record.queueTimer);
    record.queueTimer = undefined;
  }

  #clearFrameDeadline(record: TaskRecord): void {
    if (record.frameTimer !== undefined) clearTimeout(record.frameTimer);
    record.frameTimer = undefined;
  }

  #clearGroupDeadline(record: TaskRecord): void {
    if (record.groupTimer !== undefined) clearTimeout(record.groupTimer);
    record.groupTimer = undefined;
  }

  /** The same live answer the scheduler will read, at the moment it really reads it. */
  #wantedNow(record: TaskRecord, continuation: boolean): boolean {
    if (record.doneResolved) return false;
    const predicate = continuation ? record.wantedContinuation : record.wanted;
    return predicate === undefined ? true : predicate();
  }

  #taskOf(record: TaskRecord): OutboundTask {
    const task: OutboundTask = {
      done: record.done,
      failure: () => record.failure,
      outcome: () => this.#outcome(record),
      release: () => this.#release(record),
      setWanted: (wanted: () => boolean) => {
        record.wanted = wanted;
      },
      settled: () => record.doneResolved,
      state: () => record.state,
      withdraw: () => this.#withdraw(record),
    };
    record.task = task;
    return task;
  }

  #withdraw(record: TaskRecord): boolean {
    if (record.writtenFrames > 0) return false;
    return record.withdrawNow?.() === true;
  }

  #outcome(record: TaskRecord): OutboundTaskOutcome {
    return {
      refusedBeforeWrite: record.state === 'refused',
      timedOutPhysically: record.physicallyStuck,
      // "Taken back before anything of it was built" is a statement about FRAMES, not about the ending:
      // a stream whose continuation deadline expires has really written every chunk before it, so it
      // must never report a withdrawal that never happened.
      withdrewBeforeWrite: record.state === 'withdrawn' && record.writtenFrames === 0,
      wroteAnything: record.writtenFrames > 0,
    };
  }

  /**
   * ONE exit path for every way a task can end - written, withdrawn, refused at build time, failed in
   * the sink, or settled by a deadline. It is idempotent, which is what makes timeout-vs-write and
   * retirement-vs-write settle exactly once, and it gives back every real resource this TASK holds: its
   * source reservation, its secret copy, its scheduler references and all three timers.
   *
   * WHAT IT DELIBERATELY DOES NOT GIVE BACK: the working bytes of a frame whose write promise has not
   * settled. Those belong to that write, not to this task, and the native copy of the frame is still
   * being read by the addon - `#settleFrameWorking` is the only way they come back.
   */
  #finish(
    record: TaskRecord,
    ending: OutboundTaskState,
    reason: Error | undefined,
    connectionUnrecoverable: boolean,
  ): boolean {
    if (record.doneResolved) return false;
    record.doneResolved = true;
    record.state = ending;
    if (ending === 'failed' || ending === 'refused') record.failure = reason;
    this.#records.delete(record);
    const outcome = this.#outcome(record);
    if (outcome.wroteAnything) this.#wroteAnything += 1;
    if (outcome.refusedBeforeWrite) this.#refusedBeforeWrite += 1;
    if (outcome.withdrewBeforeWrite) this.#withdrewBeforeWrite += 1;
    // Release the ownership THIS task still held. Bytes that a BUILT frame owns are released by their
    // own encoder - a SecretInput frame zeroes its secret as it copies it into the frame, and the
    // frame itself is zeroed by its own `release` once the sink has settled - so nothing is zeroed
    // twice, and nothing is zeroed while the native side may still be reading it.
    this.#release(record);
    record.resolveDone();
    if (record.task !== undefined) {
      this.#handlers.onSettled(record.key, { connectionUnrecoverable, outcome, reason });
    }
    return true;
  }

  /**
   * Release everything this task still holds. Idempotent; safe on a task that is still live.
   *
   * The scheduler's frame declarations go back FIRST: a cursor frame that has not been built yet keeps
   * its own reference to the whole stream source, so returning the source reservation while the
   * scheduler still held that declaration would be a bound that describes bytes nobody released.
   */
  #release(record: TaskRecord): void {
    this.#clearGroupDeadline(record);
    this.#clearFrameDeadline(record);
    this.#clearQueueDeadline(record);
    record.abandonHeld?.();
    record.secretOwnership?.release();
    // An in-flight group's frame builder and cursor still reference its source. A caller ending
    // or releasing its task cannot free those references; only the actual scheduler completion can.
    if (record.schedulerSettled) this.#releaseSourceReservation(record);
  }
}
