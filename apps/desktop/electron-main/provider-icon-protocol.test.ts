// @vitest-environment node
import { createHash } from 'node:crypto';
import type { ProviderIconReadResult } from '@aura/contracts/generated';
import { expect, test } from 'vitest';

import {
  MAX_PROVIDER_ICON_READS,
  ProviderIconProtocol,
  type ProviderIconRead,
} from './provider-icon-protocol.js';

// The catalog can show hundreds of provider icons at once. Each one is a Core read, and a burst of
// them used to fill Core's read lane and refuse everything else on it; the protocol reads them a few
// at a time and keeps the ones it has, so scrolling back or reopening reads nothing again.

const PNG = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const ARTIFACT = 'art_01ARZ3NDEKTSV4RRFFQ69G5FAV';

/** A distinct icon per index: its bytes, its hash and the address the renderer asks for. */
function icon(index: number) {
  const bytes = Buffer.concat([PNG, Buffer.from(`icon-${index}`)]);
  const hex = createHash('sha256').update(bytes).digest('hex');
  const artifact = `${ARTIFACT.slice(0, -2)}${String(index % 100).padStart(2, '0')}`;
  const result: ProviderIconReadResult = {
    byte_length: bytes.byteLength,
    content_hash: { algorithm: 'blake3', hex },
    png_base64: bytes.toString('base64'),
  };
  return { bytes, hex, path: `/provider-icon/${artifact}/${hex}.png`, result };
}

/** A Core stand-in that answers when told to and records how many reads overlapped. */
function core() {
  const icons = new Map<string, ReturnType<typeof icon>>();
  const waiting: (() => void)[] = [];
  let running = 0;
  const seen = { reads: 0, mostAtOnce: 0, fail: new Set<string>() };
  const read: ProviderIconRead = async (params) => {
    seen.reads += 1;
    running += 1;
    seen.mostAtOnce = Math.max(seen.mostAtOnce, running);
    await new Promise<void>((release) => waiting.push(release));
    running -= 1;
    if (seen.fail.delete(params.content_hash.hex)) throw new Error('ipc.handler_overloaded');
    const found = icons.get(params.content_hash.hex);
    if (found === undefined) throw new Error('not found');
    return found.result;
  };
  const add = (index: number) => {
    const made = icon(index);
    icons.set(made.hex, made);
    return made;
  };
  /** Lets every read that is waiting finish, then gives the protocol a turn to start the next. */
  const answer = async () => {
    while (waiting.length > 0) {
      for (const release of waiting.splice(0)) release();
      await new Promise((settle) => setTimeout(settle, 0));
    }
  };
  return { add, answer, read, seen };
}

test('a burst of icons reaches Core a few reads at a time and every icon is served', async () => {
  const fake = core();
  const protocol = new ProviderIconProtocol(fake.read);
  const icons = Array.from({ length: 192 }, (_, index) => fake.add(index));
  const responses = icons.map((made) => protocol.serve(made.path));
  await fake.answer();
  const served = await Promise.all(responses);
  expect(fake.seen.mostAtOnce).toBeLessThanOrEqual(MAX_PROVIDER_ICON_READS);
  expect(fake.seen.reads).toBe(192);
  for (const [index, response] of served.entries()) {
    expect(response?.status).toBe(200);
    expect(Buffer.from(await response!.arrayBuffer()).equals(icons[index]!.bytes)).toBe(true);
  }
});

test('an icon already read is served again without another Core read', async () => {
  const fake = core();
  const protocol = new ProviderIconProtocol(fake.read);
  const made = fake.add(1);
  const first = protocol.serve(made.path);
  await fake.answer();
  expect((await first)?.status).toBe(200);
  const again = await protocol.serve(made.path);
  expect(again?.status).toBe(200);
  expect(Buffer.from(await again!.arrayBuffer()).equals(made.bytes)).toBe(true);
  expect(fake.seen.reads).toBe(1);
});

test('the same icon asked for twice at once is read once', async () => {
  const fake = core();
  const protocol = new ProviderIconProtocol(fake.read);
  const made = fake.add(2);
  const both = [protocol.serve(made.path), protocol.serve(made.path)];
  await fake.answer();
  const [left, right] = await Promise.all(both);
  expect([left?.status, right?.status]).toEqual([200, 200]);
  expect(fake.seen.reads).toBe(1);
});

test('a failed read is not kept, so the icon is read again next time', async () => {
  const fake = core();
  const protocol = new ProviderIconProtocol(fake.read);
  const made = fake.add(3);
  fake.seen.fail.add(made.hex);
  const refused = protocol.serve(made.path);
  await fake.answer();
  expect((await refused)?.status).toBe(404);
  const retried = protocol.serve(made.path);
  await fake.answer();
  expect((await retried)?.status).toBe(200);
  expect(fake.seen.reads).toBe(2);
});

test('bytes that are not the requested icon are refused and not kept', async () => {
  const protocol = new ProviderIconProtocol(async () => icon(5).result);
  expect((await protocol.serve(icon(4).path))?.status).toBe(404);
  expect(protocol.cachedBytes).toBe(0);
});

test('kept icons stay inside the byte budget, the least recently shown leaving first', async () => {
  const fake = core();
  const sizes = icon(10).bytes.byteLength;
  const protocol = new ProviderIconProtocol(fake.read, { cacheBytes: sizes * 2 });
  const [a, b, c] = [fake.add(10), fake.add(11), fake.add(12)];
  for (const made of [a, b]) {
    const served = protocol.serve(made.path);
    await fake.answer();
    await served;
  }
  // Showing `a` again makes `b` the least recently shown, so `c` pushes `b` out.
  await protocol.serve(a.path);
  const third = protocol.serve(c.path);
  await fake.answer();
  await third;
  expect(protocol.cachedBytes).toBeLessThanOrEqual(sizes * 2);
  const reads = fake.seen.reads;
  await protocol.serve(a.path);
  expect(fake.seen.reads).toBe(reads);
  const reread = protocol.serve(b.path);
  await fake.answer();
  expect((await reread)?.status).toBe(200);
  expect(fake.seen.reads).toBe(reads + 1);
});

test('an address that is not a provider icon is left to the other handlers', async () => {
  const protocol = new ProviderIconProtocol(core().read);
  expect(await protocol.serve('/index.html')).toBeUndefined();
  expect(await protocol.serve(`/provider-icon/${ARTIFACT}/not-a-hash.png`)).toBeUndefined();
});
