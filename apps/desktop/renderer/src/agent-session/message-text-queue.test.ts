import { afterEach, expect, test } from 'vitest';
import { MessageTextQueue } from './message-text-queue';

let queue = new MessageTextQueue(1);

const tick = async (times = 8): Promise<void> => {
  for (let index = 0; index < times; index += 1) await Promise.resolve();
};

afterEach(() => {
  queue = new MessageTextQueue(1);
});

/** A queue-level harness. Every page registers itself and waits until the test
 *  releases it, so the order is decided by the queue and never by timing. */
function harness() {
  const started: string[] = [];
  const waiting = new Map<string, () => void>();
  const make = (name: string) => async (): Promise<void> => {
    started.push(name);
    await new Promise<void>((resolve) => waiting.set(name, resolve));
  };
  /** What the loader does for one page: register the byte and ask for it. */
  const ask = (id: string, name: string, offset = 0, visible = false): void => {
    queue.read(id, offset, make(name), visible);
  };
  return {
    ask,
    /** Release one held page by name, or every held page. */
    release: (name?: string) => {
      const names = name === undefined ? [...waiting.keys()] : [name];
      for (const key of names) {
        const resolve = waiting.get(key);
        waiting.delete(key);
        resolve?.();
      }
    },
    started,
    waitFor: async (count: number): Promise<void> => {
      for (let index = 0; index < 40 && started.length < count; index += 1) await tick();
      expect(started.length).toBeGreaterThanOrEqual(count);
    },
  };
}

test('an on-screen body is served before an off-screen continuation', async () => {
  const { ask, release, started, waitFor } = harness();
  // The first body starts reading; the second asks for its first page, and a
  // third body nobody has touched is waiting too.
  ask('first', 'first:1');
  await waitFor(1);
  ask('second', 'second:1');
  ask('background', 'background:1');
  // The reading body asks for its next page while the second body waits.
  ask('first', 'first:2', 512);
  release('first:1');
  await waitFor(2);
  // With one free slot the continuation goes before the untouched background
  // body, even though that body arrived earlier.
  expect(started).toEqual(['first:1', 'first:2']);

  // The second body's page is now in flight. Its next page must win over the
  // continuation the loader has just asked for.
  ask('first', 'first:3', 1024);
  ask('second', 'second:2', 512);
  queue.setVisible('second', true);
  release('first:2');
  await waitFor(3);
  expect(started).toEqual(['first:1', 'first:2', 'second:2']);

  // Everything that was waiting still runs, oldest request first.
  for (let round = 0; round < 20 && started.length < 5; round += 1) {
    release();
    await tick();
  }
  expect(started).toEqual(['first:1', 'first:2', 'second:2', 'first:3', 'background:1']);
});

test('a continuation outranks an untouched background request', async () => {
  const { ask, release, started, waitFor } = harness();
  ask('continuation', 'continuation:1');
  await waitFor(1);
  ask('background', 'background:1');
  ask('background', 'background:2');
  // The reading body asks for its next page before the untouched body starts.
  ask('continuation', 'continuation:2', 512);
  release();
  await waitFor(2);
  expect(started).toEqual(['continuation:1', 'continuation:2']);
});

test('a failed page is not retried until retry is called', async () => {
  let attempts = 0;
  const fail = async (): Promise<void> => {
    attempts += 1;
    throw new Error('读取失败');
  };
  queue.read('body', 0, fail);
  await tick();
  expect(attempts).toBe(1);
  // Re-registering and re-signalling after a failure must not re-fetch it.
  queue.read('body', 0, fail);
  queue.setVisible('body', true);
  await tick(16);
  expect(attempts).toBe(1);

  queue.retry('body');
  await tick();
  expect(attempts).toBe(2);
});

test('concurrency never exceeds its bound', async () => {
  queue = new MessageTextQueue(2);
  const { ask, release, started, waitFor } = harness();
  for (let index = 0; index < 5; index += 1) ask(`body-${index}`, `page-${index}`);
  await waitFor(2);
  expect(queue.activeCount).toBeLessThanOrEqual(2);
  for (let round = 0; round < 4; round += 1) {
    release();
    await tick(12);
    expect(queue.activeCount).toBeLessThanOrEqual(2);
  }
  expect(started).toEqual(['page-0', 'page-1', 'page-2', 'page-3', 'page-4']);
});

test('an unmounted body is dropped without blocking the queue', async () => {
  const { ask, release, started, waitFor } = harness();
  ask('gone', 'gone');
  await waitFor(1);
  ask('next', 'next');
  queue.remove('gone');
  release();
  await waitFor(2);
  expect(started).toEqual(['gone', 'next']);
  expect(queue.queuedCount).toBe(0);
});

test('a page starts from the byte the loader set, never from an earlier one', async () => {
  queue = new MessageTextQueue(1);
  const offsets: number[] = [];
  const page = async (_signal: AbortSignal, offset: number): Promise<void> => {
    offsets.push(offset);
  };
  await page(new AbortController().signal, 1024);
  queue.read('body', 1024, page);
  await tick();
  expect(offsets.at(-1)).toBe(1024);
});

test('a body that left the viewport gives up its place in front of the queue', async () => {
  const { ask, release, started, waitFor } = harness();
  ask('first', 'first:1');
  await waitFor(1);
  ask('earlier', 'earlier:1');
  ask('scrolled-to', 'scrolled-to:1');
  // Scrolling to the last body promotes it in front of the request that arrived
  // earlier …
  queue.setVisible('scrolled-to', true);
  // … and scrolling away withdraws that promotion before its page ever starts,
  // so the older request is served first again.
  queue.setVisible('scrolled-to', false);
  release('first:1');
  await waitFor(2);
  expect(started).toEqual(['first:1', 'earlier:1']);
  release();
  await waitFor(3);
  expect(started).toEqual(['first:1', 'earlier:1', 'scrolled-to:1']);
});

test('an on-screen body that is still on screen keeps its place', async () => {
  const { ask, release, started, waitFor } = harness();
  ask('first', 'first:1');
  await waitFor(1);
  ask('earlier', 'earlier:1');
  ask('scrolled-to', 'scrolled-to:1');
  queue.setVisible('scrolled-to', true);
  release('first:1');
  await waitFor(2);
  // The promotion is real: staying on screen serves this body first.
  expect(started).toEqual(['first:1', 'scrolled-to:1']);
});

test('a fresh promotion survives the page that was in flight when it arrived', async () => {
  const { ask, release, started, waitFor } = harness();
  ask('scrolling', 'scrolling:1');
  ask('background', 'background:1');
  await waitFor(1);
  // The page in flight was not promoted. The body then scrolls into view and
  // its continuation is registered: that promotion belongs to the next page.
  queue.setVisible('scrolling', false);
  queue.setVisible('scrolling', true);
  ask('scrolling', 'scrolling:2', 512);
  release('scrolling:1');
  await waitFor(2);
  expect(started).toEqual(['scrolling:1', 'scrolling:2']);
});

test('a newly registered on-screen body starts before older background work', async () => {
  const { ask, release, started, waitFor } = harness();
  ask('first', 'first:1');
  await waitFor(1);
  ask('older', 'older:1');
  // The viewport registers a body that is on screen right now: its first page
  // carries that state instead of dropping behind the request that arrived
  // earlier.
  ask('visible', 'visible:1', 0, true);
  release('first:1');
  await waitFor(2);
  expect(started).toEqual(['first:1', 'visible:1']);
  release();
  await waitFor(3);
  expect(started).toEqual(['first:1', 'visible:1', 'older:1']);
});

test('background work still starts while two on-screen bodies keep asking for pages', async () => {
  // The review's starvation probe: two visible bodies re-request a page every round,
  // so a queue that only ranks visible-first never reaches the background body at
  // all. Screen-first priority is right; starving everything else indefinitely is
  // not, so background work must make bounded progress.
  queue = new MessageTextQueue(2);
  const started: string[] = [];
  const releases = new Map<string, () => void>();
  const ask = (id: string, round: number, visible: boolean): void => {
    queue.read(
      id,
      round,
      async () => {
        const key = `${id}:${round}`;
        started.push(key);
        await new Promise<void>((resolve) => releases.set(key, resolve));
      },
      visible,
    );
  };
  ask('visible-a', 0, true);
  ask('visible-b', 0, true);
  ask('background', 0, false);
  try {
    for (let round = 0; round < 32; round += 1) {
      ask('visible-a', round + 1, true);
      ask('visible-b', round + 1, true);
      releases.get(`visible-a:${round}`)?.();
      releases.get(`visible-b:${round}`)?.();
      await tick(12);
    }
    expect(started).toContain('background:0');
  } finally {
    for (const id of ['visible-a', 'visible-b', 'background']) queue.remove(id);
    for (const resolve of releases.values()) resolve();
    await tick(12);
  }
});
