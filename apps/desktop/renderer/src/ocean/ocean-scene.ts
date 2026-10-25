import { CELL, Mosaic } from './mosaic';
import { Swimmer, type SpeciesName } from './swimmer';

/** Where each shark keeps to: phase on its ellipse, its radii as shares of the view, speeds. */
const PLAN: readonly [SpeciesName, number, number, number, number, number][] = [
  ['white', 0.3, 0.4, 0.36, 0.1, 44],
  ['leopard', 2.5, 0.34, 0.4, -0.12, 36],
  ['blue', 4.4, 0.44, 0.32, 0.085, 48],
  ['hammer', 5.6, 0.3, 0.44, -0.095, 40],
];
const TAU = Math.PI * 2;
const RIPPLE_SECONDS = 1.4;

interface Mote {
  x: number;
  y: number;
  run: number;
  glyph: string;
  rise: number;
  alpha: number;
}

export interface OceanScene {
  /** Fits the canvas to its box and lays the sharks out again. */
  resize(): void;
  /** Moves everything on by `dt` seconds (none for a still picture) and draws it. */
  frame(dt: number): void;
  pointer(at: { x: number; y: number } | undefined): void;
  /** A touch on the water: a ripple, and the sharks near it dart away. */
  poke(at: { x: number; y: number }): void;
}

/**
 * The ocean behind the empty conversation: a faint grid of water, specks drifting up,
 * and four sharks in tiles circling the middle. `undefined` where the canvas cannot draw.
 */
export function createOcean(
  canvas: HTMLCanvasElement,
  random = Math.random,
): OceanScene | undefined {
  const context = canvas.getContext('2d');
  if (!context) return undefined;
  const ctx = context;
  let width = 0;
  let height = 0;
  let mosaic = new Mosaic(1, 1);
  let swimmers: Swimmer[] = [];
  const motes: Mote[] = [];
  const ripples: { x: number; y: number; age: number }[] = [];
  let pointer: { x: number; y: number } | undefined;

  const scene: OceanScene = {
    resize() {
      const box = canvas.getBoundingClientRect();
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      width = Math.max(1, box.width);
      height = Math.max(1, box.height);
      canvas.width = Math.round(width * dpr);
      canvas.height = Math.round(height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      mosaic = new Mosaic(Math.ceil(width / CELL), Math.ceil(height / CELL));
      const scale = Math.min(width * 1.15, 1100);
      swimmers = PLAN.map(([kind, phase, rx, ry, speed, pace]) => {
        const swimmer = new Swimmer(
          kind,
          scale,
          { phase, rx: rx * width, ry: ry * height, speed, pace, wobble: phase * 3 },
          phase,
        );
        swimmer.place(width / 2, height / 2);
        return swimmer;
      });
      motes.length = 0;
      for (let i = 0; i < Math.round((width * height) / 9000); i++)
        motes.push({
          x: random() * width,
          y: random() * height,
          run: 1 + Math.floor(random() * 4),
          glyph: ".:-'"[Math.floor(random() * 4)]!,
          rise: 3 + random() * 6,
          alpha: 0.25 + random() * 0.35,
        });
    },
    frame(dt) {
      ctx.clearRect(0, 0, width, height);
      drawWater(dt);
      for (const swimmer of swimmers)
        if (dt > 0) swimmer.update(dt, width / 2, height / 2, pointer);
      drawTiles();
      drawRipples(dt);
    },
    pointer(at) {
      pointer = at;
    },
    poke(at) {
      ripples.push({ ...at, age: 0 });
      for (const swimmer of swimmers)
        if (Math.hypot(swimmer.x - at.x, swimmer.y - at.y) < swimmer.length) swimmer.flee = 1.4;
    },
  };

  function drawWater(dt: number) {
    ctx.strokeStyle = 'rgba(36, 39, 36, 0.045)';
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = 0.5; x < width; x += CELL) {
      ctx.moveTo(x, 0);
      ctx.lineTo(x, height);
    }
    for (let y = 0.5; y < height; y += CELL) {
      ctx.moveTo(0, y);
      ctx.lineTo(width, y);
    }
    ctx.stroke();
    ctx.font = "600 8px 'JetBrains Mono Variable', 'Cascadia Mono', Consolas, monospace";
    ctx.textAlign = 'center';
    ctx.textBaseline = 'middle';
    for (const mote of motes) {
      mote.y -= mote.rise * dt;
      mote.x += Math.sin(mote.y / 40) * 0.08;
      if (mote.y < -10) {
        mote.y = height + 10;
        mote.x = random() * width;
      }
      ctx.fillStyle = `rgba(120, 126, 121, ${mote.alpha})`;
      for (let k = 0; k < mote.run; k++)
        ctx.fillText(
          mote.glyph,
          Math.round(mote.x / CELL) * CELL + CELL / 2,
          (Math.round(mote.y / CELL) + k) * CELL + CELL / 2,
        );
    }
  }

  function drawTiles() {
    ctx.font = "600 7.5px 'JetBrains Mono Variable', 'Cascadia Mono', Consolas, monospace";
    for (const tile of mosaic.tiles(swimmers)) {
      const x = tile.col * CELL;
      const y = tile.row * CELL;
      ctx.globalAlpha = tile.alpha;
      ctx.fillStyle = tile.fill;
      ctx.fillRect(x + 1, y + 1, CELL - 2, CELL - 2);
      ctx.fillStyle = tile.ink;
      ctx.fillText(tile.glyph, x + CELL / 2, y + CELL / 2 + 0.5);
    }
    ctx.globalAlpha = 1;
  }

  function drawRipples(dt: number) {
    for (const ripple of ripples) {
      ripple.age += dt;
      const p = ripple.age / RIPPLE_SECONDS;
      const radius = 18 + p * 60;
      ctx.strokeStyle = `rgba(63, 98, 92, ${0.32 * (1 - p)})`;
      ctx.lineWidth = 1.2;
      ctx.beginPath();
      ctx.ellipse(ripple.x, ripple.y, radius, radius * 0.5, 0, 0, TAU);
      ctx.stroke();
    }
    for (let i = ripples.length - 1; i >= 0; i--)
      if (ripples[i]!.age > RIPPLE_SECONDS) ripples.splice(i, 1);
  }

  scene.resize();
  return scene;
}
