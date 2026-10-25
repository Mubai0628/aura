import { isRecord } from './rpc.js';

/**
 * The binary streams Core is sending this channel.
 * Each open stream has the next chunk sequence it expects and the byte offset reached, so a chunk out
 * of order fails the channel and a disconnect can report where every stream would resume.
 */
export class InboundStreams {
  readonly #streamSequences = new Map<string, bigint>();
  readonly #streamOffsets = new Map<string, bigint>();

  /** Accept the next chunk of an open stream, or throw for an unknown, duplicate or missing one. */
  chunk(frame: { chunkSequence: bigint; data: Buffer; streamId: string }): void {
    const expected = this.#streamSequences.get(frame.streamId);
    if (expected === undefined || frame.chunkSequence !== expected) {
      throw new Error('binary stream contained an unknown, duplicate, or missing chunk sequence');
    }
    this.#streamSequences.set(frame.streamId, expected + 1n);
    this.#streamOffsets.set(
      frame.streamId,
      (this.#streamOffsets.get(frame.streamId) ?? 0n) + BigInt(frame.data.byteLength),
    );
  }

  open(params: unknown): void {
    if (
      !isRecord(params) ||
      typeof params.stream_id !== 'string' ||
      this.#streamSequences.has(params.stream_id)
    ) {
      throw new Error('invalid or duplicate stream.open');
    }
    this.#streamSequences.set(params.stream_id, 0n);
    const offset =
      typeof params.offset === 'string' && /^\d+$/u.test(params.offset)
        ? BigInt(params.offset)
        : typeof params.offset === 'number' && Number.isSafeInteger(params.offset)
          ? BigInt(params.offset)
          : 0n;
    this.#streamOffsets.set(params.stream_id, offset);
  }

  close(params: unknown): void {
    if (
      !isRecord(params) ||
      typeof params.stream_id !== 'string' ||
      !this.#streamSequences.delete(params.stream_id)
    ) {
      throw new Error('stream.close referenced an unknown stream');
    }
    this.#streamOffsets.delete(params.stream_id);
  }

  /** Where every open stream would resume: its next chunk and byte offset. */
  resumeCursors(): { next_chunk: string; offset: string; stream_id: string }[] {
    return [...this.#streamSequences].map(([streamId, nextChunk]) => ({
      next_chunk: nextChunk.toString(),
      offset: (this.#streamOffsets.get(streamId) ?? 0n).toString(),
      stream_id: streamId,
    }));
  }
}
