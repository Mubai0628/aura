/**
 * Aura's own input on a model page. Everything goes through DevTools to the page alone: the
 * user's mouse never moves and their keyboard is never used. While it runs, the page's input
 * events are marked as Aura's, so the user's own touch is still told apart.
 */
import type { PageDebugger } from './cdp.js';
import type { Point } from './elements.js';
import type { KeyStroke } from './keys.js';

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

async function synthetic<T>(devtools: PageDebugger, work: () => Promise<T>): Promise<T> {
  devtools.synthetic += 1;
  try {
    return await work();
  } finally {
    // Input events arrive a moment after their command returns.
    setTimeout(() => {
      devtools.synthetic -= 1;
    }, 150);
  }
}

/** A brief mark where Aura acts, for the user watching the page. */
export function mark(devtools: PageDebugger, point: Point): void {
  void devtools
    .send('Overlay.highlightRect', {
      x: Math.round(point.x - 14),
      y: Math.round(point.y - 14),
      width: 28,
      height: 28,
      color: { r: 54, g: 199, b: 255, a: 0.35 },
      outlineColor: { r: 0, g: 140, b: 210, a: 1 },
    })
    .then(() => sleep(700))
    .then(() => devtools.send('Overlay.hideHighlight'))
    .catch(() => undefined);
}

/** `page` is in page coordinates; the events go in the view's, `scale` times smaller. */
export function click(
  devtools: PageDebugger,
  page: Point,
  scale: number,
  button: 'left' | 'right' | 'middle',
  double: boolean,
): Promise<void> {
  const x = page.x * scale;
  const y = page.y * scale;
  return synthetic(devtools, async () => {
    await devtools.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y });
    for (const clickCount of double ? [1, 2] : [1]) {
      await devtools.send('Input.dispatchMouseEvent', {
        type: 'mousePressed',
        x,
        y,
        button,
        clickCount,
      });
      await devtools.send('Input.dispatchMouseEvent', {
        type: 'mouseReleased',
        x,
        y,
        button,
        clickCount,
      });
    }
  });
}

export function hover(devtools: PageDebugger, page: Point, scale: number): Promise<void> {
  return synthetic(devtools, () =>
    devtools
      .send('Input.dispatchMouseEvent', {
        type: 'mouseMoved',
        x: page.x * scale,
        y: page.y * scale,
      })
      .then(() => undefined),
  );
}

export function wheel(
  devtools: PageDebugger,
  page: Point,
  scale: number,
  dx: number,
  dy: number,
): Promise<void> {
  return synthetic(devtools, () =>
    devtools
      .send('Input.dispatchMouseEvent', {
        type: 'mouseWheel',
        x: page.x * scale,
        y: page.y * scale,
        deltaX: dx,
        deltaY: dy,
      })
      .then(() => undefined),
  );
}

export function press(devtools: PageDebugger, stroke: KeyStroke, holdMs: number): Promise<void> {
  const { modifiers, key, code, windowsVirtualKeyCode, text } = stroke;
  const base = {
    modifiers,
    key,
    code,
    windowsVirtualKeyCode,
    nativeVirtualKeyCode: windowsVirtualKeyCode,
  };
  return synthetic(devtools, async () => {
    await devtools.send('Input.dispatchKeyEvent', {
      ...base,
      type: text === undefined ? 'rawKeyDown' : 'keyDown',
      ...(text === undefined ? {} : { text, unmodifiedText: text }),
    });
    if (holdMs > 0) await sleep(holdMs);
    await devtools.send('Input.dispatchKeyEvent', { ...base, type: 'keyUp' });
  });
}

export function insertText(devtools: PageDebugger, text: string): Promise<void> {
  return synthetic(devtools, () =>
    devtools.send('Input.insertText', { text }).then(() => undefined),
  );
}
