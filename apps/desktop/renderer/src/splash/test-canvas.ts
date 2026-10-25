/** A canvas whose 2D context records what is written on it; jsdom has none. For the splash tests. */
export interface Written {
  text: string;
  x: number;
  y: number;
  font: string;
  alpha: number;
  /** How far it is squashed upright: 1 stands straight, less is turned away on a reel. */
  scaleY: number;
}

/** A rectangle filled on the canvas. */
export interface Filled {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Placement {
  x: number;
  y: number;
  sx: number;
  sy: number;
}

export function recordingCanvas(width = 1000, height = 600) {
  const written: Written[] = [];
  const filled: Filled[] = [];
  const saved: { font: string; globalAlpha: number; at: Placement }[] = [];
  let at: Placement = { x: 0, y: 0, sx: 1, sy: 1 };
  const context = {
    font: '',
    fillStyle: '' as string,
    strokeStyle: '' as string,
    lineWidth: 1,
    lineJoin: '',
    globalAlpha: 1,
    textAlign: '',
    textBaseline: '',
    clearRect() {},
    setTransform() {},
    fillRect(x: number, y: number, w: number, h: number) {
      filled.push({ x, y, w, h });
    },
    save() {
      saved.push({ font: context.font, globalAlpha: context.globalAlpha, at: { ...at } });
    },
    restore() {
      const last = saved.pop()!;
      context.font = last.font;
      context.globalAlpha = last.globalAlpha;
      at = last.at;
    },
    translate(dx: number, dy: number) {
      at.x += at.sx * dx;
      at.y += at.sy * dy;
    },
    scale(sx: number, sy: number) {
      at.sx *= sx;
      at.sy *= sy;
    },
    measureText: () => ({ width: 6.6 }),
    strokeText() {},
    fillText(text: string, x: number, y: number) {
      written.push({
        text,
        x: at.x + at.sx * x,
        y: at.y + at.sy * y,
        font: context.font,
        alpha: context.globalAlpha,
        scaleY: at.sy,
      });
    },
  };
  const canvas = document.createElement('canvas');
  canvas.getContext = (() => context) as unknown as typeof canvas.getContext;
  canvas.getBoundingClientRect = () => ({ width, height }) as DOMRect;
  return { canvas, written, filled };
}

/** The name's letters in one frame (drawn in the heavy face), and the water's characters. */
export const nameIn = (written: readonly Written[]) =>
  written.filter((w) => w.font.startsWith('600 '));
export const waterIn = (written: readonly Written[]) =>
  written.filter((w) => !w.font.startsWith('600 '));
