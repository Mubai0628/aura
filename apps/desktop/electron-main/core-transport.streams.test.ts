import { afterEach, describe, expect, it, vi } from 'vitest';
import { fixture } from './core-transport.fixture.js';
import { idFromBytes } from './ids.js';

vi.mock('./main-pipe-bridge.js', () => ({ connectMainPipe: vi.fn() }));

afterEach(() => {
  vi.useRealTimers();
  vi.clearAllMocks();
});

const streamId = idFromBytes('str', new Uint8Array(16).fill(7));
const notice = (method: string, params: Record<string, unknown>) => ({
  jsonrpc: '2.0',
  method,
  params,
});

/** A channel with one stream opened at byte 100 and its first four bytes received. */
async function streaming() {
  vi.useFakeTimers();
  const f = await fixture();
  f.deliver(notice('stream.open', { stream_id: streamId, offset: '100' }));
  await vi.advanceTimersByTimeAsync(0);
  f.deliverBinary({ streamId, chunkSequence: 0n, data: Buffer.from('abcd') });
  await vi.advanceTimersByTimeAsync(0);
  return f;
}

// Core sends a large body as numbered binary chunks after `stream.open`. Main keeps each open
// stream's next chunk and byte offset: a chunk that repeats or skips one is never handed on as part
// of the body, and a lost channel says where every stream would resume.
describe('Core binary streams', () => {
  it('chunks in order are handed on, and a lost channel says where the stream resumes', async () => {
    const f = await streaming();
    f.deliverBinary({ streamId, chunkSequence: 1n, data: Buffer.from('ef') });
    await vi.advanceTimersByTimeAsync(0);
    expect(f.onBinaryChunk).toHaveBeenCalledTimes(2);
    f.transport.close();
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'ipc.transport_lost',
        details: {
          binary_resume_cursors: [{ next_chunk: '2', offset: '106', stream_id: streamId }],
        },
      }),
    );
  });

  it.each([
    ['repeats', 0n],
    ['skips', 2n],
  ])('a chunk that %s a sequence fails the channel and is not handed on', async (_, sequence) => {
    const f = await streaming();
    f.deliverBinary({ streamId, chunkSequence: sequence, data: Buffer.from('efgh') });
    await vi.advanceTimersByTimeAsync(0);
    expect(f.onBinaryChunk).toHaveBeenCalledTimes(1);
    expect(f.onClosed).toHaveBeenCalledOnce();
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'ipc.protocol_violation',
        message: expect.stringContaining('chunk sequence'),
        details: {
          binary_resume_cursors: [{ next_chunk: '1', offset: '104', stream_id: streamId }],
        },
      }),
    );
  });

  it('a chunk after the stream closed fails the channel', async () => {
    const f = await streaming();
    f.deliver(notice('stream.close', { stream_id: streamId }));
    await vi.advanceTimersByTimeAsync(0);
    f.deliverBinary({ streamId, chunkSequence: 1n, data: Buffer.from('ef') });
    await vi.advanceTimersByTimeAsync(0);
    expect(f.onBinaryChunk).toHaveBeenCalledTimes(1);
    expect(f.onClosed).toHaveBeenCalledWith(
      expect.objectContaining({
        code: 'ipc.protocol_violation',
        details: { binary_resume_cursors: [] },
      }),
    );
  });
});
