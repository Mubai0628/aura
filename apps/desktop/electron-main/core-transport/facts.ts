import type { FrameSchedulerFacts } from '../frame-scheduler.js';
import type { OutboundSendOwnerFacts } from '../outbound-send-owner.js';
import type {
  CommandInvocationId,
  CorrelationId,
  IpcErrorEnvelope,
} from '@aura/contracts/generated';
import type { RequestStage } from '../request-registry.js';

/** What the transport reports about its lifecycle and channel, and its handlers. */
/**
 * One identity-only record of a request's lifecycle on this channel.
 *
 * WHAT IT MAY CARRY, and nothing else: the correlation id, the CommandInvocationId, the invocation
 * (method) that was really sent, the lifecycle stage, and the channel id / connection epoch this
 * transport really authenticated. `params`, `secret`, `nonce` and every other field of the request are
 * NOT copied: this type has no member for them, so they cannot reach an observer by accident.
 *
 * WHICH STAGES ARE REPORTED, and where: `pending-write` and `submitted` at the two send-stage points
 * (`#requestInternal`), and `late-terminal-consumed` at the one place where this transport consumes the
 * first real terminal of a request its caller had already given up on (`TransportLifecycleSeam.lateTerminalConsumed`).
 * The third is a lifecycle fact about a request, not a send stage, and it is emitted AFTER the terminal
 * was consumed - it cannot settle, resolve or answer anything.
 *
 * WHO CONSUMES IT: an acceptance-layer observer only. Nothing in the product registers one, and the
 * record is not an input to any decision - it cannot settle, reorder or answer a request.
 */
export interface CoreTransportLifecycleRecord {
  readonly channel_id: string;
  readonly command_invocation_id?: CommandInvocationId;
  readonly connection_epoch: number;
  readonly method: string;
  readonly request_id: CorrelationId;
  readonly stage: RequestStage;
}

/** What this transport can honestly say about its own lifecycle seam. Counts only, never content. */
export interface CoreTransportChannelFacts {
  readonly authenticated: boolean;
  readonly channel_id: string;
  readonly closed: boolean;
  readonly connection_epoch: number;
  readonly observer_attached: boolean;
}

export interface CoreTransportHandlers {
  onBinaryChunk: (chunk: {
    chunkSequence: bigint;
    data: Buffer;
    endOfMessage: boolean;
    streamId: string;
  }) => void;
  onClosed: (error: IpcErrorEnvelope) => void;
  onNotification: (method: string, params: unknown) => void;
}

/** The transport's request, outbound and observer counters (`CoreTransport.requestStats`). */
export interface CoreTransportRequestStats {
  active: number;
  activeControl: number;
  admissionRefusals: number;
  audit: number;
  controlAdmissionRefusals: number;
  duplicates: number;
  evicted: number;
  late: number;
  lifecycleFailures: number;
  lifecycleSeen: number;
  outbound: OutboundSendOwnerFacts;
  /** Frames and bytes really waiting in the scheduler, with the peaks a run reached. */
  outboundFrames: FrameSchedulerFacts;
  unresolved: number;
  unresolvedControl: number;
  unsentTerminals: number;
  /** Frames the client refused to send because the caller had already given up on them. */
  withdrawn: number;
}

/** How the lifecycle observer seam itself behaved (`CoreTransport.lifecycleFacts`). */
export interface TransportLifecycleFacts {
  failures: number;
  last_failure: string | null;
  observer_attached: boolean;
  seen: number;
}
