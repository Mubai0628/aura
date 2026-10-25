import { MAX_CONTROL_PAYLOAD_BYTES } from '../protocol.js';
import {
  type FrameWanted,
  MAX_P0_FRAME_BYTES,
  type QueuePriority,
  type ScheduledFrame,
} from './requests.js';

/** The scheduler's capacity limits, its reported facts and its bucket accounting. */
/** How many frames and bytes may wait, per priority class. Counts are slots; bytes are real bytes. */
export interface FrameSchedulerLimits {
  readonly bulkBytes: number;
  readonly bulkFrames: number;
  readonly p0Bytes: number;
  readonly p0Frames: number;
  readonly sharedBytes: number;
  readonly sharedFrames: number;
}

/**
 * The scheduler's own bound on the frames it has COMMITTED to write.
 *
 * SCOPE, deliberately narrow: it is the sum of the frames holding a place in a queue, the cursor frames
 * held between two of their own chunks, and the frame inside the sink. It does NOT cover the source
 * data a stream is a cursor over - that source is held by the frame's owner for the task's whole life
 * and is bounded there - and it does not cover the copy a sink makes of the frame it is writing. It is a
 * frame-queue pool, not a file-size limit and not a statement about the process.
 */
export const MAX_QUEUED_BYTES = 16 * 1024 * 1024;

export const P0_RESERVED_FRAMES = 8;

export const P0_RESERVED_BYTES = MAX_P0_FRAME_BYTES;

export const SHARED_MAX_FRAMES = 64 - P0_RESERVED_FRAMES;

export const SHARED_MAX_BYTES = MAX_QUEUED_BYTES - P0_RESERVED_BYTES;

export const P1_RESERVED_FRAMES = 8;

/** One maximum catalog control frame: the payload plus its length, kind and authenticated header. */
export const P1_RESERVED_BYTES = MAX_CONTROL_PAYLOAD_BYTES + 4 + 1 + 32;

export const BULK_MAX_FRAMES = SHARED_MAX_FRAMES - P1_RESERVED_FRAMES;

export const BULK_MAX_BYTES = SHARED_MAX_BYTES - P1_RESERVED_BYTES;

/** The production bounds, derived from the protocol frame constants. */
export const FRAME_SCHEDULER_LIMITS: FrameSchedulerLimits = {
  bulkBytes: BULK_MAX_BYTES,
  bulkFrames: BULK_MAX_FRAMES,
  p0Bytes: P0_RESERVED_BYTES,
  p0Frames: P0_RESERVED_FRAMES,
  sharedBytes: SHARED_MAX_BYTES,
  sharedFrames: SHARED_MAX_FRAMES,
};

/**
 * What the scheduler really did, for a test or a diagnostic. Counts and bytes only.
 *
 * Every bucket is reported separately and `heldBytes` is their sum, because the four of them have
 * different owners: a task waiting for a place in a queue (`pending`) is not the same thing as a
 * frame holding a place (`queued`), a stream cursor frame held between two chunks (`staged`) is not
 * in a queue at all, and the frame inside the sink (`inFlight`) is memory the native side is reading.
 * A single "queued" number cannot show any of that, and a bound derived from it would not be a bound
 * on what the process really holds.
 */
export interface FrameSchedulerFacts {
  readonly bytesWritten: number;
  readonly closed: boolean;
  /** Frames this scheduler has committed to write: queued + staged + in flight. The bounded part. */
  readonly committedBytes: number;
  readonly committedFrames: number;
  /** Everything it has been asked to keep alive, pending list included. */
  readonly heldBytes: number;
  readonly heldFrames: number;
  readonly inFlight: boolean;
  readonly inFlightBytes: number;
  readonly inFlightFrames: number;
  /** The next sequence number this channel would stamp. Compared by a reader to prove gap-freedom. */
  readonly nextSequence: string;
  readonly peakCommittedBytes: number;
  readonly peakHeldBytes: number;
  readonly peakHeldFrames: number;
  readonly peakPendingBytes: number;
  readonly peakPendingGroups: number;
  readonly peakQueuedBytes: number;
  readonly peakQueuedFrames: number;
  readonly peakQueuedGroups: number;
  readonly peakStagedBytes: number;
  /** Frames waiting for a place in a queue, held by `#pendingGroups`. */
  readonly pendingBytes: number;
  readonly pendingFrames: number;
  readonly pendingGroups: number;
  /** Frames holding a place in a queue. */
  readonly queuedBytes: number;
  readonly queuedFrames: number;
  readonly queuedGroups: number;
  /** Stream cursor frames held out of the queue between two of their own chunks. */
  readonly stagedBytes: number;
  readonly stagedFrames: number;
  readonly stagedGroups: number;
  readonly withdrawnGroups: number;
  readonly writesFailed: number;
  readonly writesStarted: number;
  readonly writesSucceeded: number;
}

export interface Occupancy {
  bytes: number;
  frames: number;
  groups: number;
}

export type BucketName = 'pending' | 'queued' | 'staged';

export type Placement = 'none' | BucketName;

/** How one scheduling turn for one group ended. `refused` and `failed` are never collapsed. */
export type WriteOutcome =
  | { kind: 'written' }
  | { kind: 'staged' }
  | { kind: 'refused'; reason: Error }
  | { kind: 'failed'; reason: Error };

export const emptyOccupancy = (): Occupancy => ({ bytes: 0, frames: 0, groups: 0 });

export interface ScheduledGroup {
  readonly kind: 'atomic' | 'stream';
  readonly label: string;
  readonly onFrameQueued: ((index: number) => void) | undefined;
  readonly onFrameWriteSettled: ((index: number, sizeBytes: number) => void) | undefined;
  readonly onFrameWriteStarted: ((index: number, sizeBytes: number) => void) | undefined;
  readonly priority: QueuePriority;
  /** Asked for the FIRST frame only; a later frame of an atomic group is an obligation, not a wish. */
  readonly wanted: FrameWanted | undefined;
  /** Asked for the frames a STREAM builds after its first one. */
  readonly wantedContinuation: FrameWanted | undefined;
  /** Frames not written yet. An atomic group holds them all; a cursor holds exactly one. */
  readonly frames: ScheduledFrame[];
  /** A stream's next chunk; `undefined` for an atomic group, which declared all of its frames. */
  readonly next: (() => ScheduledFrame | undefined) | undefined;
  /** How many frames of this group have really been handed to the sink. */
  frameIndex: number;
  reject: (reason: Error) => void;
  resolve: () => void;
  withdrawn: boolean;
}

export const frameBytes = (frames: readonly ScheduledFrame[]): number =>
  frames.reduce((total, frame) => total + frame.sizeBytes, 0);

export const SHARED_PRIORITIES: readonly QueuePriority[] = [1, 2, 3];

export const BULK_PRIORITIES: readonly QueuePriority[] = [2, 3];

export const SHARED_SCHEDULE: readonly QueuePriority[] = [1, 1, 1, 1, 2, 2, 3];

export const freshBuckets = (): Occupancy[] => [
  emptyOccupancy(),
  emptyOccupancy(),
  emptyOccupancy(),
  emptyOccupancy(),
];

export const bucketTotal = (bucket: Occupancy[], pick: keyof Occupancy): number =>
  bucket.reduce((total, entry) => total + entry[pick], 0);

export const sumIn = (
  bucket: Occupancy[],
  priorities: readonly QueuePriority[],
  pick: keyof Occupancy,
): number =>
  priorities.reduce<number>((total, priority) => {
    const entry = bucket[priority];
    return entry === undefined ? total : total + entry[pick];
  }, 0);
