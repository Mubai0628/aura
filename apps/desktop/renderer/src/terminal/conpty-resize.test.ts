import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { conptyResizer, type TerminalSize } from './conpty-resize';

class FakeTerminal {
  cols = 80;
  rows = 24;
  constructor(private readonly events: string[]) {}
  resize(cols: number, rows: number) {
    this.events.push(`resize ${cols}x${rows}`);
    this.cols = cols;
    this.rows = rows;
  }
}

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

function fixture(initial: TerminalSize | undefined, send?: (size: TerminalSize) => Promise<void>) {
  const events: string[] = [];
  const terminal = new FakeTerminal(events);
  const state = { size: initial, dragging: false };
  const sent: TerminalSize[] = [];
  const dragEnds = new Set<() => void>();
  const resizer = conptyResizer(terminal, {
    measure: () => state.size,
    drain: async () => {
      events.push('drain');
    },
    hold: () => {
      events.push('hold');
      return {
        repaint: async () => {
          events.push('repaint');
          return new TextEncoder().encode('\x1b[H');
        },
        release: () => {
          events.push('release');
        },
      };
    },
    send:
      send ??
      (async (size) => {
        events.push(`send ${size.cols}x${size.rows}`);
        sent.push(size);
      }),
    apply: async (output) => {
      events.push(`apply ${output.length}`);
    },
    dragging: () => state.dragging,
    onDragEnd: (listener) => {
      dragEnds.add(listener);
      return () => {
        dragEnds.delete(listener);
      };
    },
  });
  const endDrag = () => {
    state.dragging = false;
    for (const listener of dragEnds) listener();
  };
  return { events, state, sent, resizer, endDrag, dragEnds };
}

it('applies the first size at once and a dragged size once, after the panel settles', async () => {
  const { events, state, sent, resizer } = fixture({ cols: 100, rows: 30 });
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  expect(sent).toEqual([{ cols: 100, rows: 30 }]);
  for (const cols of [90, 70, 50, 34]) {
    state.size = { cols, rows: 30 };
    resizer.request();
    await vi.advanceTimersByTimeAsync(16);
  }
  expect(sent).toHaveLength(1);
  await vi.advanceTimersByTimeAsync(200);
  expect(sent).toEqual([
    { cols: 100, rows: 30 },
    { cols: 34, rows: 30 },
  ]);
  // Output already received renders first; output after the resize is held back until ConPTY's
  // repaint has arrived and is then written lined up with xterm's rows.
  const cycle = (size: string) => [
    'drain',
    'hold',
    `resize ${size}`,
    `send ${size}`,
    'repaint',
    'apply 3',
    'release',
  ];
  expect(events).toEqual([...cycle('100x30'), ...cycle('34x30')]);
  resizer.dispose();
});

it('does not resize xterm again until ConPTY applied the previous size', async () => {
  let release: (() => void) | undefined;
  const { events, state, resizer } = fixture(
    { cols: 100, rows: 30 },
    () =>
      new Promise<void>((resolve) => {
        release = resolve;
      }),
  );
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  state.size = { cols: 40, rows: 30 };
  resizer.request();
  await vi.advanceTimersByTimeAsync(200);
  expect(events.filter((event) => event.startsWith('resize'))).toEqual(['resize 100x30']);
  release?.();
  await vi.advanceTimersByTimeAsync(0);
  expect(events.filter((event) => event.startsWith('resize'))).toEqual([
    'resize 100x30',
    'resize 40x30',
  ]);
  release?.();
  resizer.dispose();
});

it('waits for the divider to be released and then resizes once, to the final size', async () => {
  const { state, sent, resizer, endDrag } = fixture({ cols: 100, rows: 30 });
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  state.dragging = true;
  // The owner's drag: to the narrowest, held there, back out, still pressed.
  for (const cols of [70, 34, 34, 34, 60, 96]) {
    state.size = { cols, rows: 30 };
    resizer.request();
    await vi.advanceTimersByTimeAsync(300);
  }
  expect(sent).toEqual([{ cols: 100, rows: 30 }]);
  endDrag();
  await vi.advanceTimersByTimeAsync(200);
  expect(sent).toEqual([
    { cols: 100, rows: 30 },
    { cols: 96, rows: 30 },
  ]);
  resizer.dispose();
});

it('does not resize a timer that fires during a drag', async () => {
  const { state, sent, resizer, endDrag } = fixture({ cols: 100, rows: 30 });
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  state.size = { cols: 60, rows: 30 };
  resizer.request();
  state.dragging = true;
  await vi.advanceTimersByTimeAsync(500);
  expect(sent).toHaveLength(1);
  endDrag();
  await vi.advanceTimersByTimeAsync(200);
  expect(sent.at(-1)).toEqual({ cols: 60, rows: 30 });
  resizer.dispose();
});

it('ignores the sizes of a collapsing or hidden panel', async () => {
  const { state, sent, resizer } = fixture({ cols: 100, rows: 30 });
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  // Past the snap point the owner's terminal was resized to 2 columns.
  for (const size of [
    { cols: 2, rows: 30 },
    { cols: 19, rows: 30 },
    { cols: 60, rows: 3 },
  ]) {
    state.size = size;
    resizer.request();
    await vi.advanceTimersByTimeAsync(200);
  }
  expect(sent).toEqual([{ cols: 100, rows: 30 }]);
  resizer.dispose();
});

it('writes the held output even when the panel unmounts while ConPTY repaints', async () => {
  const applied: number[] = [];
  let arrive: ((output: Uint8Array) => void) | undefined;
  let released = false;
  const resizer = conptyResizer(new FakeTerminal([]), {
    measure: () => ({ cols: 100, rows: 30 }),
    drain: async () => undefined,
    hold: () => ({
      repaint: () =>
        new Promise<Uint8Array>((resolve) => {
          arrive = resolve;
        }),
      release: () => {
        released = true;
      },
    }),
    send: async () => undefined,
    apply: async (output) => {
      applied.push(output.length);
    },
    dragging: () => false,
    onDragEnd: () => () => undefined,
  });
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  resizer.dispose();
  arrive?.(new Uint8Array(5));
  await vi.advanceTimersByTimeAsync(0);
  expect(applied).toEqual([5]);
  expect(released).toBe(true);
});

it('stops after dispose and ignores sizes that did not change or cannot be measured', async () => {
  const { state, sent, resizer, dragEnds } = fixture(undefined);
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  state.size = { cols: 60, rows: 20 };
  resizer.request();
  await vi.advanceTimersByTimeAsync(0);
  resizer.request();
  await vi.advanceTimersByTimeAsync(200);
  expect(sent).toEqual([{ cols: 60, rows: 20 }]);
  state.size = { cols: 30, rows: 20 };
  resizer.request();
  resizer.dispose();
  await vi.advanceTimersByTimeAsync(200);
  expect(sent).toEqual([{ cols: 60, rows: 20 }]);
  expect(dragEnds.size).toBe(0);
});
