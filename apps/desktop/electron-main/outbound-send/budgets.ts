import { MAX_P0_FRAME_BYTES, type ScheduledFrame } from '../frame-scheduler.js';
import { MAX_CONTROL_PAYLOAD_BYTES, MAX_SECRET_INPUT_BYTES } from '../protocol.js';

/** Frame sizes and the byte and time budgets of outbound sending. */
/** The exact byte size of one control frame carrying a payload of this size. */
export const controlFrameSize = (payloadBytes: number): number => payloadBytes + 1 + 32 + 4;

/** The exact byte size of one SecretInput frame carrying a secret of this size. */
export const secretInputFrameSize = (secretBytes: number): number => 1 + 32 + 40 + secretBytes + 4;

/** The exact byte size of one binary chunk frame carrying this much data. */
export const binaryFrameSize = (dataBytes: number): number => 1 + 32 + 25 + dataBytes + 4;

export const sizedFrame = (
  sizeBytes: number,
  build: () => Buffer,
  release?: (bytes: Buffer) => void,
): ScheduledFrame => (release === undefined ? { build, sizeBytes } : { build, release, sizeBytes });

/**
 * How long ONE COMPLETE FRAME may stay inside the sink before this connection is retired.
 *
 * WHY IT IS INDEPENDENT: a caller's request budget says how long a CALLER waits, not how long a byte
 * stream may stay unprovable. When a write cannot finish, the client can no longer show whether half
 * a frame is on the wire, and a framed stream cannot be re-synchronised - so the honest answer is to
 * retire THIS connection and report the possibly-executed outcome, never `not_sent`.
 *
 * WHY THIS SIZE: it is an order of magnitude above the atomic SecretInput binding budget (10 s), which
 * is itself far above a normal enqueue, and it is longer than the default request budget (15 s), so
 * one slow request can never retire a channel by itself. It fires only on a channel that has stopped
 * draining altogether. A test that has to observe retirement inside a fake clock injects a shorter
 * budget through `CoreTransport.connect`.
 */
export const PHYSICAL_SEND_BUDGET_MS = 120_000;

/**
 * How long the atomic "command control frame + SecretInput frame" binding may take once its command
 * frame has been committed.
 *
 * After that point the Core may already be holding half of the operation, and a framed stream cannot
 * be re-synchronised, so a binding that cannot finish inside this bound is a channel that can no
 * longer continue honestly: it is retired rather than silently abandoned.
 */
export const SECRET_INPUT_BINDING_BUDGET_MS = 10_000;

/**
 * The largest file the retired P07 Tool Draft policy accepted.
 *
 * It is not a new product limit and not a send-layer cap: `TOOL_PACKAGE_IMPORT_POLICY_V1.max_file_bytes`
 * is 64 MiB. Draft staging was the only bulk-stream sender; it was retired, and the Core now refuses
 * binary frames, so no production path sends a stream. The bulk class keeps this size until the stream
 * layer is removed or reused - a business check, and nothing here, is what refuses an over-policy file.
 */
export const MAX_TOOL_DRAFT_FILE_BYTES = 64 * 1024 * 1024;

/**
 * The SOURCE budget of one priority class: bytes of source data this owner keeps for a task's life.
 *
 * WHAT IT COUNTS, per live task, for the whole life of that task: the serialised JSON-RPC payload of a
 * registered request, a bulk stream's whole source buffer, and the send-owned SecretInput copy. It is
 * reserved BEFORE anything of the task exists, so the sum of the reservations is an upper bound on the
 * source bytes this module retains - and a task is either fully admitted or refused while it is still
 * provably unsent.
 *
 * WHY IT IS SPLIT BY CLASS: the classes exist so that saturating one cannot refuse another. A saturated
 * bulk budget must never be able to refuse the P0 control class that drains it, and a 64 MiB Tool Draft
 * upload must never be able to refuse the maximum catalog control frame. Each class is sized from the
 * largest legitimate source of THAT class, so a single legitimate file is never permanently
 * un-admittable: only another bulk stream can use the bulk class up.
 *
 * WHAT IT DELIBERATELY DOES NOT COUNT: the frame bodies. A built frame and the native copy of it are
 * held by different owners (the scheduler and the addon) for a much shorter time and are bounded by
 * `MAX_FRAME_WORKING_BYTES` instead. Counting them here is exactly the defect this split removes.
 */
export const P0_SOURCE_BUDGET_BYTES = MAX_P0_FRAME_BYTES + MAX_SECRET_INPUT_BYTES;

/**
 * The ordinary/control class. Sized as two maximum catalog-sized control sources: one is the catalog
 * frame itself, and the second is the headroom that keeps ordinary requests (a few hundred bytes each)
 * admissible while a catalog frame is in flight. It is deliberately NOT one maximum frame, which would
 * let a single legal catalog response refuse every ordinary request behind it.
 */
export const CONTROL_SOURCE_BUDGET_BYTES = 2 * MAX_CONTROL_PAYLOAD_BYTES + MAX_SECRET_INPUT_BYTES;

/** The bulk class: one file the existing Tool Draft policy accepts, plus one SecretInput copy. */
export const STREAM_SOURCE_BUDGET_BYTES = MAX_TOOL_DRAFT_FILE_BYTES + MAX_SECRET_INPUT_BYTES;

/** Every class's source budget, and the whole-channel source bound they compose into. */
export const MAX_RETAINED_SOURCE_BYTES =
  P0_SOURCE_BUDGET_BYTES + CONTROL_SOURCE_BUDGET_BYTES + STREAM_SOURCE_BUDGET_BYTES;

/**
 * The largest frame any encoder in `protocol.ts` can produce: a control frame carrying the maximum
 * 6 MiB control payload. A binary chunk is 256 KiB and a SecretInput frame ~2.6 KiB, so this bounds all
 * three.
 */
export const MAX_ENCODED_FRAME_BYTES = controlFrameSize(MAX_CONTROL_PAYLOAD_BYTES);

/**
 * The FRAME WRITE WORKING budget: the bytes of the one encoded frame that is really being written.
 *
 * WHAT IT COUNTS, at the same instant: the JS frame the scheduler built and handed to the sink, and the
 * copy `apps/desktop/native/aura-main-pipe/src/addon.cpp::Write` makes of it (`operation->bytes.assign`)
 * before it returns its promise. The native copy is held until the asynchronous write completes, which
 * is why the reservation is taken BEFORE the sink call and settled on the real write promise - not when
 * the caller stops waiting.
 *
 * WHY TWO FRAME BODIES: the sink is strictly serialised, so at most one frame is inside it at a time and
 * at most one native copy exists at a time; a frame plus its copy is therefore the whole peak. The
 * budget is sized from the largest frame the protocol can produce so it can never be short: a channel
 * that could not take the working bytes of a frame would have to refuse a frame it had already built.
 *
 * WHAT IT DOES NOT COUNT: source data (that is `MAX_RETAINED_SOURCE_BYTES`), scheduler frame
 * DECLARATIONS that hold no bytes yet, and anything outside this send path.
 */
export const MAX_FRAME_WORKING_BYTES = 2 * MAX_ENCODED_FRAME_BYTES;

/**
 * The most bytes this owner may hold for ONE channel, across every task it owns at once.
 *
 * It is the SUM of the two scopes above and nothing else: source data retained for a task's life, plus
 * the encoded frame and native copy in flight. Both halves are enforced separately (a class slice for
 * sources, the single in-flight frame for working bytes), so this figure is a bound and not a queue
 * limit - it is deliberately not called a file-size cap, and it says nothing about whole-process RSS.
 */
export const MAX_RETAINED_SEND_BYTES = MAX_RETAINED_SOURCE_BYTES + MAX_FRAME_WORKING_BYTES;

/**
 * A task was refused because admitting it would take its SOURCE CLASS past that class's bound.
 *
 * It is a LOCAL, TRANSIENT capacity refusal with nothing on the wire - no frame was built and no
 * sequence number was spent - so the transport reports it as `not_sent` and records the reason. It says
 * nothing about whether the bytes were legal: a legitimate file that arrives while the class is busy is
 * refused here and admitted once the class drains, which is what keeps a capacity limit distinguishable
 * from the business check that refuses an over-policy or unencodable payload for good.
 */
export class SendBudgetExceededError extends Error {
  /** Which source class was full. */
  readonly sourceClass: 'p0' | 'control' | 'stream';
  /** Source bytes that class already held when the task was measured. */
  readonly heldBytes: number;
  /** The class's own bound. */
  readonly limitBytes: number;
  /** Source bytes this task needed. */
  readonly neededBytes: number;
  /** This refusal is a capacity fact, not a verdict about the data: the same task may be admitted later. */
  readonly transient = true;

  constructor(
    sourceClass: 'p0' | 'control' | 'stream',
    heldBytes: number,
    neededBytes: number,
    limitBytes: number,
  ) {
    super(
      `the ${sourceClass} source class already holds ${heldBytes} of ${limitBytes} retained source ` +
        `bytes; this task needs ${neededBytes} more`,
    );
    this.name = 'SendBudgetExceededError';
    this.sourceClass = sourceClass;
    this.heldBytes = heldBytes;
    this.limitBytes = limitBytes;
    this.neededBytes = neededBytes;
  }
}
