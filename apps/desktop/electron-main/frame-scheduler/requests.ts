import type { Buffer } from 'node:buffer';

/** What callers hand the frame scheduler, and the errors it settles them with. */
/** What the scheduler writes to: the channel's physical connection, or a controlled fixture. */
export interface AsyncFrameSink {
  write(bytes: Buffer): Promise<void>;
}

export type QueuePriority = 0 | 1 | 2 | 3;

/**
 * The one bound a P0 frame is measured against before anything is queued.
 *
 * It is exported because the OWNER of a frame has to enforce it too: a frame rejected here must be
 * rejected while it is still provably unsent, so that it neither reaches the sink nor spends an
 * outbound sequence number.
 */
export const MAX_P0_FRAME_BYTES = 32 * 1024;

export const p0FrameIsOversized = (bytes: number): boolean => bytes > MAX_P0_FRAME_BYTES;

/**
 * How many complete frames ONE ATOMIC GROUP may hold. It is two, and it stays two.
 *
 * This is the limit that makes "command + SecretInput" indivisible. It deliberately does NOT bound a
 * binary stream: a stream is not an atomic group, it is a cursor that exposes one frame at a time, so
 * its total chunk count never reaches this number.
 */
export const MAX_FRAMES_PER_GROUP = 2;

/**
 * One frame an owner asks the scheduler to write.
 *
 * `sizeBytes` is the size the frame WILL have. It is declared up front - and required to be exact by
 * the scheduler - because queue capacity, the byte bound and the P0 frame limit are all decided
 * BEFORE the frame is built, while the bytes themselves are produced at the scheduling boundary by
 * `build`. That split is what lets the sequence number be committed in true send order without ever
 * handing out a number to a frame that is later withdrawn: `build` is called at most once, exactly
 * when the scheduler has chosen this frame to be the next one written.
 */
export interface ScheduledFrame {
  readonly build: () => Buffer;
  /**
   * Release the bytes `build` produced, once the sink is finished with them - successfully or not.
   *
   * Only the frame's OWNER knows whether those bytes are secret, so the scheduler never guesses: it
   * drops its own reference and calls this exactly once per built frame.
   */
  readonly release?: (bytes: Buffer) => void;
  readonly sizeBytes: number;
}

/** Asked once per frame, at the scheduling boundary and BEFORE the frame is built. */
export type FrameWanted = () => boolean;

export interface FrameRequestBase {
  /**
   * Called when frame `index` of this group becomes the owner's resident business: it is now waiting
   * for the wire (a place in a queue, or the turn of the cursor that holds it). It is where a per
   * frame QUEUE deadline starts.
   */
  readonly onFrameQueued?: (index: number) => void;
  /**
   * Called synchronously at the scheduling boundary, immediately before this frame is handed to the
   * sink. It is where the frame's owner records "this write has begun", which is the ONE point that
   * makes a later failure possibly-partial rather than provably unsent - and where a per write
   * deadline starts.
   *
   * `sizeBytes` is the exact size of the frame that is about to be written, so an owner that has to
   * account for the bytes a write really holds (the frame plus whatever the sink copies out of it) can
   * do so without re-deriving it, and always BEFORE the sink call.
   */
  readonly onFrameWriteStarted?: (index: number, sizeBytes: number) => void;
  /**
   * Called once the write of frame `index` has settled, whether it succeeded or failed. It is where a
   * per write deadline ends, so a later frame gets a deadline of its own rather than inheriting the
   * elapsed time of the frames before it - and where the working bytes `onFrameWriteStarted` reserved
   * are given back, on the REAL write promise rather than when the caller stopped waiting.
   */
  readonly onFrameWriteSettled?: (index: number, sizeBytes: number) => void;
  /** A label for diagnostics only. It never carries params, a body or a secret. */
  readonly label: string;
  readonly priority: QueuePriority;
  /**
   * Asked once for the FIRST frame, at the scheduling boundary and BEFORE it is built.
   *
   * A frame whose owner says "no" is refused while nothing of it exists: it is not built, it spends no
   * sequence number, no byte reaches the sink, and the channel stays healthy. That is what makes
   * "reject the waiter, then let the write dispatch anyway with no notice" impossible even when the
   * frame would have been written later than the withdrawal that ended its request.
   */
  readonly wanted?: FrameWanted;
}

/**
 * An atomic group: every frame it will ever write, declared at once, at most `MAX_FRAMES_PER_GROUP`.
 *
 * There is deliberately no continuation predicate here. Once the first frame of the group has been
 * handed to the sink the Core may already hold half of the operation, so the remaining frames are an
 * obligation the group has already taken on: they are written back to back, and a group that cannot
 * finish is a channel that can no longer continue honestly - not a group that is half-withdrawn.
 */
export interface AtomicFrameGroupRequest extends FrameRequestBase {
  readonly frames: readonly ScheduledFrame[];
  readonly kind: 'atomic';
}

/** A stream cursor: unbounded in total length, exactly one frame exposed at a time. */
export interface FrameStreamRequest extends FrameRequestBase {
  readonly first: ScheduledFrame;
  readonly kind: 'stream';
  /**
   * The next chunk's frame, or `undefined` when the stream has no more chunks.
   *
   * Called only after the previous chunk's write has SETTLED, so the cursor itself is what keeps at
   * most one frame of a stream alive.
   */
  readonly next: () => ScheduledFrame | undefined;
  /**
   * The same question as `wanted`, for the chunks built after the first one.
   *
   * It is separate because the first chunk is often the commitment itself - the frame a caller is
   * already waiting on - while the chunks that follow it are the ones an abandoned stream can still
   * stop feeding to a wire nobody reads.
   */
  readonly wantedContinuation?: FrameWanted;
}

export type FrameEnqueueRequest = AtomicFrameGroupRequest | FrameStreamRequest;

/**
 * A local, request-scoped refusal of one payload.
 *
 * It means the bytes could not even be built, so nothing of this frame reached the sink: the frame
 * must be reported as `not_sent` and must not spend an outbound sequence number. It is deliberately
 * distinct from a sink failure, which may have written half a frame.
 */
export class FrameBuildError extends Error {
  readonly buildCause: unknown;

  constructor(cause: unknown) {
    super(cause instanceof Error ? cause.message : String(cause));
    this.name = 'FrameBuildError';
    this.buildCause = cause;
  }
}

/** The frame was refused before it was built, because it could never fit its own class's bound. */
export class FrameTooLargeError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'FrameTooLargeError';
  }
}

/** The channel was retired while this frame was still queued. It was never written. */
export class FrameQueueClosedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'FrameQueueClosedError';
  }
}

/** The channel was retired while this frame was being written, so the stream may hold half of it. */
export class FrameWriteAbortedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'FrameWriteAbortedError';
  }
}

/**
 * The channel retired while this frame was queued, or the frame was taken back at the boundary.
 * Either way nothing of it was written and its bytes are released; the waiting owner decides whether
 * its own budget has expired and how to report that.
 */
export class FrameWithdrawnError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'FrameWithdrawnError';
  }
}
