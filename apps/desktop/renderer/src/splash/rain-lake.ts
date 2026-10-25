import { hash } from '../ocean/mosaic';
import { Rain, depthAt, seeded, surface, type View } from './rain';

/** The water from calm to churned; a crest is drawn in the accent. */
const WATER = ' .·-~=+*#%';
/** What the water shows before it settles, in its first half second. */
const NOISE = '01<>/\\|{}[]*+=~';
const LETTERS = 'ABCDEFGHJKMNPRSTUVWXYZ';
export const NAME = 'AURA';
const CREST = '#4f9a80';
const INK = '#242724';
/** A thin rim of the screen's own white around each letter of the name, so it reads over the rain. */
const RIM = '#fafbf9';
const MONO = "'JetBrains Mono Variable', 'Cascadia Mono', Consolas, monospace";
const SIZE = 11;
const LINE = 1.15;
/**
 * Each letter of the name is on
 * a reel of its own. The reels start one after another from the left, spin and slow down, and each
 * stops on its letter, so the name is whole before `SPLASH_INTRO_MS`.
 */
const REEL_AT = 0.12;
const REEL_STEP = 0.16;
const REEL_SPIN = 0.45;
/** Letters on a reel ahead of its own, and the turn from one to the next. */
const REEL_LETTERS = 6;
const REEL_TURN = 0.62;
/** The moment a picture that does not move shows. */
export const STILL_AT = 2.6;

const clamp = (v: number, low = 0, high = 1) => Math.min(high, Math.max(low, v));
function mix(a: string, b: string, t: number): string {
  const channels = (hex: string) => {
    const n = Number.parseInt(hex.slice(1), 16);
    return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
  };
  const from = channels(a);
  const to = channels(b);
  return `rgb(${from.map((v, i) => Math.round(v + (to[i]! - v) * t)).join(',')})`;
}
/** Shallow to deep. */
const LAKE = Array.from({ length: 8 }, (_, i) => mix('#c9d8d3', '#2f524c', i / 7));

export interface RainLake {
  /** Fits the canvas to its box. */
  resize(): void;
  /** Draws the lake `t` seconds after the window opened; `leave` runs from 0 to 1 as it fades. */
  frame(t: number, leave?: number): void;
  /** One picture that does not move, for less motion. */
  still(): void;
}

/**
 * The startup screen: the lake in characters, each cell's character and shade from the
 * water's height there, the drops still falling as short streaks, and the name rolling in over the
 * water. A screen that is leaving dissolves cell by cell. `undefined` where the canvas cannot draw.
 */
export function createRainLake(
  canvas: HTMLCanvasElement,
  random = seeded(1),
): RainLake | undefined {
  const context = canvas.getContext('2d');
  if (!context) return undefined;
  const ctx = context;
  const rain = new Rain(random);
  // The letters each reel passes before its own; fixed, so the rain's sequence is the rain's alone.
  const reels = [...NAME].map((own, i) => [
    ...Array.from(
      { length: REEL_LETTERS },
      (_, k) => LETTERS[Math.floor(hash(i + 101, k) * LETTERS.length)]!,
    ),
    own,
  ]);
  const view: View = { w: 1, h: 1 };
  const font = `${SIZE}px ${MONO}`;
  let cw = 7;
  let ch = Math.round(SIZE * LINE);
  let cols = 1;
  let rows = 1;
  // Each colour is set once a frame: the cells are drawn by colour.
  const groups = new Map<string, { x: number; y: number; glyph: string }[]>();
  const add = (fill: string, x: number, y: number, glyph: string) => {
    let group = groups.get(fill);
    if (!group) groups.set(fill, (group = []));
    group.push({ x, y, glyph });
  };

  function draw(t: number, leave: number, still: boolean) {
    ctx.clearRect(0, 0, view.w, view.h);
    ctx.font = font;
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'center';
    for (let row = 0; row < rows; row++) {
      const y = (row + 0.5) * ch;
      const depth = depthAt(y, view);
      for (let col = 0; col < cols; col++) {
        const x = (col + 0.5) * cw;
        const seed = hash(col, row);
        const h = surface(x, y, t, rain.drops, view);
        const v = clamp(Math.abs(h) * 0.95);
        if ((v < 0.07 && seed > 0.35) || (leave > 0 && seed < leave)) continue;
        if (!still && t < 0.05 + seed * 0.45) {
          if (v >= 0.25) add(LAKE[1]!, x, y, NOISE[Math.floor(random() * NOISE.length)]!);
          continue;
        }
        const level = Math.min(WATER.length - 1, Math.max(1, Math.round(v * (WATER.length - 1))));
        add(
          h > 0.55 ? CREST : LAKE[Math.min(7, Math.round(v * 10 * depth + 1))]!,
          x,
          y,
          WATER[level]!,
        );
      }
    }
    for (const drop of rain.falling) {
      const fallen = clamp((t - drop.at) / (drop.land - drop.at));
      add(LAKE[5]!, drop.x, drop.y - (1 - fallen) * view.h * 0.18, fallen < 0.6 ? '|' : "'");
    }
    for (const [fill, group] of groups) {
      ctx.fillStyle = fill;
      for (const cell of group) ctx.fillText(cell.glyph, cell.x, cell.y);
    }
    groups.clear();
    drawName(t, leave, still);
  }

  /**
   * The name over the water, with nothing behind it. A reel is a drum seen side on: the letter in
   * front stands upright, the ones above and below are turned away, squashed and faint, and once it
   * stops only its own letter is left.
   */
  function drawName(t: number, leave: number, still: boolean) {
    const r = Math.min(view.w, view.h);
    const size = Math.round(r * 0.07);
    const gap = r * 0.085;
    const y = view.h * 0.47;
    const radius = (size * 1.2) / REEL_TURN;
    ctx.save();
    ctx.font = `600 ${size}px ${MONO}`;
    ctx.lineJoin = 'round';
    ctx.lineWidth = Math.max(2, size * 0.1);
    ctx.strokeStyle = RIM;
    for (let i = 0; i < NAME.length; i++) {
      const spun = still ? 1 : clamp((t - REEL_AT - i * REEL_STEP) / REEL_SPIN);
      if (spun <= 0) continue;
      const x = view.w / 2 + (i - (NAME.length - 1) / 2) * gap;
      // Fast at first, slowing until its own letter is in front.
      const turned = REEL_LETTERS * (1 - (1 - spun) ** 3);
      const reel = reels[i]!;
      for (let k = Math.max(0, Math.ceil(turned - 2.5)); k <= REEL_LETTERS; k++) {
        const angle = (k - turned) * REEL_TURN;
        if (Math.abs(angle) >= Math.PI / 2) continue;
        const facing = Math.cos(angle);
        const own = k === REEL_LETTERS;
        const alpha = facing * facing * (own ? 1 : 1 - spun) * clamp(spun * 6);
        if (alpha <= 0) continue;
        ctx.globalAlpha = (1 - leave) * alpha;
        ctx.save();
        ctx.translate(x, y - radius * Math.sin(angle));
        ctx.scale(1, facing);
        ctx.strokeText(reel[k]!, 0, 0);
        ctx.fillStyle = own ? INK : LAKE[5]!;
        ctx.fillText(reel[k]!, 0, 0);
        ctx.restore();
      }
    }
    ctx.restore();
  }

  const lake: RainLake = {
    resize() {
      const box = canvas.getBoundingClientRect();
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      view.w = Math.max(1, box.width);
      view.h = Math.max(1, box.height);
      canvas.width = Math.round(view.w * dpr);
      canvas.height = Math.round(view.h * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.font = font;
      cw = ctx.measureText('M').width || 7;
      ch = Math.round(SIZE * LINE);
      cols = Math.ceil(view.w / cw);
      rows = Math.ceil(view.h / ch);
    },
    frame(t, leave = 0) {
      rain.update(t, view, leave > 0);
      draw(t, leave, false);
    },
    still() {
      rain.scatter(STILL_AT, view);
      draw(STILL_AT, 0, true);
    },
  };
  lake.resize();
  return lake;
}
