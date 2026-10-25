import type { IWindowsPty } from '@xterm/xterm';
import { onPaneDragEnd, paneDragging } from '../pane-drag';

// Aura supports Windows 11 only, whose inbox ConPTY reflows its
// screen on resize (build 21376 and later). xterm then keeps its own reflow on and, like
// ConPTY, adds rows at the bottom instead of pulling scrollback back into view.
export const CONPTY: IWindowsPty = { backend: 'conpty', buildNumber: 22_000 };
const SETTLE_MS = 120;
// Below this the panel is collapsing or hidden. Resizing ConPTY to a sliver keeps almost nothing
// of its screen.
const MIN_COLS = 20;
const MIN_ROWS = 4;

export interface ResizableTerminal {
  readonly cols: number;
  readonly rows: number;
  resize(cols: number, rows: number): void;
}
export interface TerminalSize {
  cols: number;
  rows: number;
}
export interface HeldOutput {
  // Resolves with the output held back since `hold`, once ConPTY's repaint has arrived.
  repaint(): Promise<Uint8Array>;
  release(): void;
}
export interface ConptyIo {
  measure(): TerminalSize | undefined;
  // Resolves once xterm has rendered all output received so far.
  drain(): Promise<void>;
  hold(): HeldOutput;
  send(size: TerminalSize): Promise<void>;
  // Writes the output that followed a resize, ConPTY's repaint included.
  apply(output: Uint8Array): Promise<void>;
  dragging?(): boolean;
  onDragEnd?(listener: () => void): () => void;
}

// ConPTY repaints its whole screen after every resize. xterm and ConPTY therefore change size
// together and once, after the panel stops moving and the divider is released: output already
// received is rendered first, at the size it was painted for, and the output after the resize is
// held back until ConPTY's repaint has arrived, so that the repaint can be lined up with xterm's
// rows. Resizing on every frame while ConPTY lagged behind left many garbled copies of the
// screen in scrollback.
export function conptyResizer(terminal: ResizableTerminal, io: ConptyIo) {
  let applied = '';
  let timer: ReturnType<typeof setTimeout> | undefined;
  let busy = false;
  let again = false;
  let deferred = false;
  let disposed = false;
  const dragging = io.dragging ?? paneDragging;
  // The first size applies even during a drag, so a new terminal opens at its real size.
  const waitForDrag = () => {
    if (!applied || !dragging()) return false;
    deferred = true;
    return true;
  };
  const resize = async (size: TerminalSize) => {
    const held = io.hold();
    try {
      terminal.resize(size.cols, size.rows);
      await io.send(size);
      // Written even if the panel unmounted meanwhile: the terminal and its output live on.
      await io.apply(await held.repaint());
    } finally {
      held.release();
    }
  };
  const run = async () => {
    if (disposed || waitForDrag()) return;
    if (busy) {
      again = true;
      return;
    }
    busy = true;
    try {
      await io.drain();
      const size = disposed ? undefined : io.measure();
      const key = size ? `${size.cols}:${size.rows}` : applied;
      if (size && size.cols >= MIN_COLS && size.rows >= MIN_ROWS && key !== applied) {
        applied = key;
        await resize(size);
      }
    } finally {
      busy = false;
    }
    if (again) {
      again = false;
      await run();
    }
  };
  const request = () => {
    if (disposed || waitForDrag()) return;
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(
      () => {
        timer = undefined;
        void run();
      },
      applied ? SETTLE_MS : 0,
    );
  };
  const stopWatching = (io.onDragEnd ?? onPaneDragEnd)(() => {
    if (!deferred) return;
    deferred = false;
    request();
  });
  return {
    request,
    dispose() {
      disposed = true;
      stopWatching();
      if (timer !== undefined) clearTimeout(timer);
    },
  };
}
