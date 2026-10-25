import { describe, expect, it } from 'vitest';

import { LineCounter, SnapshotLines, jsonLines } from './json-lines';

// The snapshot's JSON is counted first and then laid
// out a slice at a time, exactly as JSON.stringify(value, null, 2) lays it out.

const samples: unknown[] = [
  null,
  'text',
  7,
  true,
  [],
  {},
  [1, 'two', null, [], {}, [3], { a: 1 }],
  {
    id: 'ctx_01',
    nested: { list: [{ x: 1, y: [true, false] }, 'say "hi"\n'], empty: {} },
    count: 0,
    skipped: undefined,
    unicode: '会话 🙂',
  },
];

describe('the snapshot JSON', () => {
  it.each(samples.map((value) => [JSON.stringify(value) ?? 'undefined', value]))(
    'is laid out as JSON.stringify lays it out: %s',
    (_name, value) => {
      const expected = JSON.stringify(value, null, 2).split('\n');
      expect([...jsonLines(value)]).toEqual(expected);
      const counter = new LineCounter(value);
      while (!counter.run(1));
      expect(counter.count).toBe(expected.length);
    },
  );

  it('counts and lays out in slices, however small', () => {
    const value = { items: Array.from({ length: 500 }, (_, index) => ({ index, text: 'x' })) };
    const counter = new LineCounter(value);
    let slices = 0;
    while (!counter.run(7)) slices += 1;
    expect(slices).toBeGreaterThan(10);
    expect(counter.count).toBe(JSON.stringify(value, null, 2).split('\n').length);
  });
});

describe('the work of one snapshot', () => {
  it('knows its line count before any line, and then fills in', () => {
    const value = { items: Array.from({ length: 3_000 }, (_, index) => ({ index })) };
    const expected = JSON.stringify(value, null, 2).split('\n');
    const work = new SnapshotLines(value);
    // One tick of the clock per question: every slice is cut short.
    let clock = 0;
    const now = () => clock++;
    work.step(clock + 1, now);
    expect(work.total).toBeUndefined();
    while (work.total === undefined) work.step(clock + 1, now);
    expect(work.total).toBe(expected.length);
    expect(work.ready).toBe(0);
    while (!work.done) work.step(clock + 2, now);
    expect(work.ready).toBe(expected.length);
    expect(work.line(0)).toBe('{');
    expect(work.line(expected.length - 1)).toBe('}');
    expect(work.line(7)).toBe(expected[7]);
  });
});
