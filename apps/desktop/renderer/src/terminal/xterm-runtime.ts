import { userMessage } from '../agent-session/user-message';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import {
  attachTerminalConsumer,
  configureTerminalRenderer,
  terminalApi,
  terminalFailure,
  type TerminalRecord,
} from './store';
import { adoptTerminalStyles } from './csp-styles';
import { CONPTY, conptyResizer, type HeldOutput } from './conpty-resize';
import { applyRepaint, repaintComplete } from './conpty-align';
import { terminalPalette, terminalTheme } from './terminal-colors';
import { VtColorConverter } from './vt-colors';
import '@xterm/xterm/css/xterm.css';

interface Engine {
  terminal: Terminal;
  fit: FitAddon;
  element: HTMLDivElement;
  // Resolves once xterm has rendered all output received so far.
  drain: () => Promise<void>;
  // Keeps output back until ConPTY's repaint after a resize has arrived.
  hold: () => HeldOutput;
  dispose: () => void;
}
// ConPTY's repaint usually arrives within tens of milliseconds of the resize. Without its end
// marker (an application can hide the cursor), the output is taken once it has been quiet.
const REPAINT_FIRST_MS = 400;
const REPAINT_QUIET_MS = 80;
const REPAINT_MAX_MS = 800;
const engines = new Map<string, Engine>();
export function terminalEngine(record: TerminalRecord): Engine {
  const cached = engines.get(record.view.terminal_id);
  if (cached) return cached;
  const element = document.createElement('div');
  element.className = 'terminal-screen';
  const style = getComputedStyle(document.documentElement);
  const theme = terminalTheme({
    background: style.getPropertyValue('--canvas').trim(),
    foreground: style.getPropertyValue('--ink').trim(),
    cursor: style.getPropertyValue('--accent').trim(),
    selectionBackground: style.getPropertyValue('--accent-soft').trim(),
  });
  const colors = new VtColorConverter(terminalPalette(theme));
  const terminal = new Terminal({
    cols: 80,
    rows: 24,
    scrollback: 10000,
    fontSize: 13,
    fontFamily: 'Consolas, monospace',
    allowProposedApi: false,
    screenReaderMode: true,
    windowsPty: CONPTY,
    // ConPTY reflows the cursor line too; xterm has to lay it out the same way for ConPTY's
    // repaint to line up with its rows (conpty-align.ts).
    reflowCursorLine: true,
    theme,
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(element);
  const releaseStyles = adoptTerminalStyles(element.querySelector('.xterm-screen') ?? element);
  let pending: Uint8Array[] = [];
  let frame: number | undefined;
  let held: (() => void) | undefined;
  const joined = () => {
    const all = new Uint8Array(pending.reduce((count, item) => count + item.length, 0));
    let offset = 0;
    for (const item of pending) {
      all.set(item, offset);
      offset += item.length;
    }
    return all;
  };
  const take = () => {
    if (frame !== undefined) cancelAnimationFrame(frame);
    frame = undefined;
    const all = joined();
    pending = [];
    return all;
  };
  const schedule = () => {
    frame ??= requestAnimationFrame(() => terminal.write(take()));
  };
  attachTerminalConsumer(record, (data) => {
    // Transform display bytes only. Stored offsets and model-visible records retain raw output.
    // This ingress also feeds held ConPTY repaints and their scratch emulator in conpty-align.
    const converted = colors.write(data);
    if (!converted.length) return;
    pending.push(converted);
    if (held) held();
    else schedule();
  });
  // Output that keeps arriving while xterm renders is taken a few rounds over, not forever.
  const drain = async () => {
    for (let round = 0; round < 3; round++) {
      const data = take();
      await new Promise<void>((resolve) => {
        terminal.write(data, resolve);
      });
      if (!pending.length) return;
    }
  };
  const hold = (): HeldOutput => {
    let wake = () => {};
    held = () => wake();
    return {
      repaint: () =>
        new Promise<Uint8Array>((resolve) => {
          let quiet: ReturnType<typeof setTimeout> | undefined;
          let done = false;
          const finish = () => {
            if (done) return;
            done = true;
            clearTimeout(quiet);
            clearTimeout(limit);
            wake = () => {};
            resolve(take());
          };
          const limit = setTimeout(finish, REPAINT_MAX_MS);
          wake = () => {
            if (repaintComplete(joined())) return finish();
            clearTimeout(quiet);
            quiet = setTimeout(finish, REPAINT_QUIET_MS);
          };
          quiet = setTimeout(finish, REPAINT_FIRST_MS);
          if (pending.length) wake();
        }),
      release: () => {
        held = undefined;
        if (pending.length) schedule();
      },
    };
  };
  const send = async (input: string, interrupt = false) => {
    if (record.view.end) return;
    // Cursor reports are emulator protocol responses, not user keystrokes.
    const cursorReply = input.charCodeAt(0) === 27 && /^\[\d{1,4};\d{1,4}R$/.test(input.slice(1));
    if (record.view.controller === 'agent' && !cursorReply) return;
    const result = await terminalApi()?.write({
      session_id: record.view.session_id,
      terminal_id: record.view.terminal_id,
      input,
      interrupt,
    });
    if (result && !result.ok)
      terminalFailure(record, userMessage(result.error, '终端操作没有完成，请重试。'));
  };
  const data = terminal.onData((value) => {
    void send(value);
  });
  // Copy and paste use the browser's own clipboard events, which xterm handles and which need no
  // clipboard permission. Main refuses those permissions to every page.
  terminal.attachCustomKeyEventHandler((event) => {
    if (event.type !== 'keydown') return true;
    event.stopPropagation();
    if (event.ctrlKey && event.key.toLowerCase() === 'c') {
      const textarea = terminal.textarea;
      if (terminal.hasSelection() && textarea) {
        // The browser copies only from a selected text field; xterm's copy handler then writes
        // its own selection, as for a right-click copy. The field is emptied once copied.
        textarea.value = terminal.getSelection();
        textarea.select();
        setTimeout(() => {
          textarea.value = '';
        });
        return false;
      }
      void send('', true);
      event.preventDefault();
      return false;
    }
    // Ctrl+V: the browser's paste reaches xterm's paste handler, bracketed paste included.
    if (event.ctrlKey && event.key.toLowerCase() === 'v') return false;
    return true;
  });
  const engine: Engine = {
    terminal,
    fit,
    element,
    drain,
    hold,
    dispose: () => {
      if (frame !== undefined) cancelAnimationFrame(frame);
      data.dispose();
      terminal.dispose();
      releaseStyles();
      element.remove();
      delete record.accept;
    },
  };
  engines.set(record.view.terminal_id, engine);
  return engine;
}
configureTerminalRenderer((record) => {
  terminalEngine(record);
});
export function disposeTerminalEngine(id: string) {
  engines.get(id)?.dispose();
  engines.delete(id);
}
export function mountTerminal(record: TerminalRecord, host: HTMLDivElement) {
  const engine = terminalEngine(record);
  host.appendChild(engine.element);
  let alive = true;
  const resizer = conptyResizer(engine.terminal, {
    measure: () =>
      alive && host.clientWidth && host.clientHeight ? engine.fit.proposeDimensions() : undefined,
    drain: engine.drain,
    hold: engine.hold,
    send: async (size) => {
      if (!alive || record.view.end) return;
      const result = await terminalApi()?.resize({
        session_id: record.view.session_id,
        terminal_id: record.view.terminal_id,
        columns: size.cols,
        rows: size.rows,
      });
      if (result && !result.ok)
        terminalFailure(record, userMessage(result.error, '终端操作没有完成，请重试。'));
    },
    // A closed terminal has nothing left to write to.
    apply: async (output) => {
      if (engines.get(record.view.terminal_id) === engine)
        await applyRepaint(engine.terminal, output);
    },
  });
  const observer = new ResizeObserver(() => resizer.request());
  observer.observe(host);
  resizer.request();
  return () => {
    alive = false;
    observer.disconnect();
    resizer.dispose();
    engine.element.remove();
  };
}
