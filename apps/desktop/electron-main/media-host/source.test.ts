// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { MediaSource } from './source.js';
import { READ_BYTES, type MediaCommand } from './types.js';

const command = (size: number): MediaCommand => ({
  command_id: 'mcm-fixture-command',
  runtime_instance_id: 'runtime-fixture',
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
  file_size: size,
  media_type: 'video/webm',
  timeout_ms: 30_000,
  action: { kind: 'probe' },
});

function fixture(size = 300_000) {
  const bytes = Buffer.from(Array.from({ length: size }, (_, index) => index % 251));
  const core = {
    requestWithBudget: vi.fn(async (_method: string, value: unknown) => {
      const read = value as { offset: number; length: number };
      return {
        data_base64: bytes.subarray(read.offset, read.offset + read.length).toString('base64'),
      };
    }),
  };
  const source = new MediaSource(command(size), core, Date.now() + 30_000);
  return { bytes, core, source };
}

test('streams the same validated handle in bounded authenticated reads, without any path', async () => {
  const { bytes, core, source } = fixture();
  const response = await source.serve(new Request(source.url));
  expect(response.status).toBe(200);
  expect(response.headers.get('content-length')).toBe(String(bytes.length));
  expect(response.headers.get('accept-ranges')).toBe('bytes');
  expect(Buffer.from(await response.arrayBuffer())).toEqual(bytes);
  expect(core.requestWithBudget).toHaveBeenCalledTimes(3);
  for (const [method, params] of core.requestWithBudget.mock.calls) {
    expect(method).toBe('media.host.read');
    expect(Object.keys(params as object).sort()).toEqual([
      'command_id',
      'length',
      'offset',
      'runtime_instance_id',
      'session_id',
    ]);
    expect((params as { length: number }).length).toBeLessThanOrEqual(READ_BYTES);
  }
});

test.each([
  ['bytes=2-7', 2, 7],
  ['bytes=299990-', 299990, 299999],
  ['bytes=-5', 299995, 299999],
])('serves Chromium range %s with complete 206 headers', async (range, start, end) => {
  const { bytes, source } = fixture();
  const response = await source.serve(new Request(source.url, { headers: { range } }));
  expect(response.status).toBe(206);
  expect(response.headers.get('content-range')).toBe(`bytes ${start}-${end}/${bytes.length}`);
  expect(response.headers.get('content-length')).toBe(String(end - start + 1));
  expect(Buffer.from(await response.arrayBuffer())).toEqual(bytes.subarray(start, end + 1));
});

test.each(['bytes=300000-', 'bytes=8-2', 'bytes=-0', 'bytes=0-1,5-6'])(
  'refuses invalid range %s before reading',
  async (range) => {
    const { core, source } = fixture();
    const response = await source.serve(new Request(source.url, { headers: { range } }));
    expect(response.status).toBe(416);
    expect(response.headers.get('content-range')).toBe('bytes */300000');
    expect(core.requestWithBudget).not.toHaveBeenCalled();
  },
);

test('rejects a changed token, an arbitrary path, and a completed command explicitly', async () => {
  const { core, source } = fixture();
  for (const url of [
    source.url.replace('/source', '/other.webm'),
    'file:///C:/secret.webm',
    'aura://media-decoder/guessed/source',
  ]) {
    const response = await source.serve(new Request(url));
    expect(response.status).toBe(403);
    expect(await response.text()).toContain('拒绝');
  }
  source.close();
  expect((await source.serve(new Request(source.url))).status).toBe(403);
  expect(core.requestWithBudget).not.toHaveBeenCalled();
});

test('rejects short or malformed data instead of feeding it to the decoder', async () => {
  const { core, source } = fixture();
  core.requestWithBudget.mockResolvedValueOnce({ data_base64: '@@@@' });
  const response = await source.serve(new Request(source.url));
  await expect(response.arrayBuffer()).rejects.toThrow();
});
