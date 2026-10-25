import type { Rgb, Swimmer } from './swimmer';

/** The side of one tile, in CSS pixels. */
export const CELL = 11;
/** Characters from light to dense; a darker tile carries a denser one. */
const RAMP = '.:-=+*tSX%#@';

export interface Tile {
  col: number;
  row: number;
  /** The tile's colour, as `rgb(...)`. */
  fill: string;
  /** Fins and the body's edge are fainter. */
  alpha: number;
  glyph: string;
  /** The character's colour: dark on a light tile, light on a dark one. */
  ink: string;
}

const clamp = (v: number, low = 0, high = 1) => Math.min(high, Math.max(low, v));
const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];
/** A stable pseudo-random number in [0, 1) for a pair of integers. */
export function hash(x: number, y: number): number {
  let h = (x * 374761393 + y * 668265263) | 0;
  h = Math.imul(h ^ (h >>> 13), 1274126177);
  return ((h ^ (h >>> 16)) >>> 0) / 4294967295;
}

/**
 * Lays swimmers onto a grid of tiles: each tile near a body takes the nearest point of that body's
 * spine (how far along it, how far out, which side), then is shaded as a rounded back lit from the
 * top left, with the species' spots and a little per-tile grain, like a photograph in tiles.
 */
export class Mosaic {
  private nearest: Float32Array;
  private owner: Int16Array;
  private along: Float32Array;
  private across: Float32Array;
  private dirX: Float32Array;
  private dirY: Float32Array;
  private stamp: Int32Array;
  private frame = 0;

  constructor(
    readonly cols: number,
    readonly rows: number,
  ) {
    const n = cols * rows;
    this.nearest = new Float32Array(n);
    this.owner = new Int16Array(n);
    this.along = new Float32Array(n);
    this.across = new Float32Array(n);
    this.dirX = new Float32Array(n);
    this.dirY = new Float32Array(n);
    this.stamp = new Int32Array(n);
  }

  tiles(swimmers: readonly Swimmer[]): Tile[] {
    this.frame += 1;
    const touched = this.lay(swimmers);
    const tiles: Tile[] = [];
    for (const index of touched) {
      const tile = this.shade(index, swimmers);
      if (tile) tiles.push(tile);
    }
    return tiles;
  }

  private lay(swimmers: readonly Swimmer[]): number[] {
    const touched: number[] = [];
    swimmers.forEach((swimmer, which) => {
      const points = swimmer.spine();
      for (let i = 0; i < points.length - 1; i++) {
        const a = points[i]!;
        const b = points[i + 1]!;
        const reach = Math.max(swimmer.reach(a.u).all, swimmer.reach(b.u).all) + CELL;
        const c0 = Math.max(0, Math.floor((Math.min(a.x, b.x) - reach) / CELL));
        const c1 = Math.min(this.cols - 1, Math.floor((Math.max(a.x, b.x) + reach) / CELL));
        const r0 = Math.max(0, Math.floor((Math.min(a.y, b.y) - reach) / CELL));
        const r1 = Math.min(this.rows - 1, Math.floor((Math.max(a.y, b.y) + reach) / CELL));
        const ex = b.x - a.x;
        const ey = b.y - a.y;
        const length2 = ex * ex + ey * ey || 1e-6;
        const length = Math.sqrt(length2);
        for (let row = r0; row <= r1; row++) {
          for (let col = c0; col <= c1; col++) {
            const px = (col + 0.5) * CELL;
            const py = (row + 0.5) * CELL;
            const t = clamp(((px - a.x) * ex + (py - a.y) * ey) / length2);
            const distance = Math.hypot(px - (a.x + ex * t), py - (a.y + ey * t));
            const index = row * this.cols + col;
            if (this.stamp[index] !== this.frame) {
              this.stamp[index] = this.frame;
              this.nearest[index] = Infinity;
              touched.push(index);
            }
            if (distance < this.nearest[index]!) {
              this.nearest[index] = distance;
              this.owner[index] = which;
              this.along[index] = a.u + (b.u - a.u) * t;
              const side = ex * (py - a.y) - ey * (px - a.x) >= 0 ? 1 : -1;
              this.across[index] = side * distance;
              this.dirX[index] = ex / length;
              this.dirY[index] = ey / length;
            }
          }
        }
      }
    });
    return touched;
  }

  private shade(index: number, swimmers: readonly Swimmer[]): Tile | undefined {
    const swimmer = swimmers[this.owner[index]!]!;
    const u = this.along[index]!;
    const d = this.across[index]!;
    const { body, all } = swimmer.reach(u);
    const out = Math.abs(d);
    if (out > all || all <= 0.5) return undefined;
    const col = index % this.cols;
    const row = Math.floor(index / this.cols);
    const onFin = out > body;
    const x = onFin ? clamp((out - body) / Math.max(all - body, 1)) : out / Math.max(body, 1);
    // A rounded body lit from the top left: its normal tilts toward the tile's side of the spine.
    const side = Math.sign(d) || 1;
    const nx = -this.dirY[index]! * side;
    const ny = this.dirX[index]! * side;
    const tilt = onFin ? 0.35 : x;
    const nz = Math.sqrt(Math.max(0, 1 - tilt * tilt));
    const diffuse = Math.max(0, nx * tilt * -0.52 + ny * tilt * -0.6 + nz * 0.6);
    const s = swimmer.species;
    let colour = onFin
      ? mix(s.back, s.flank, 0.55 + x * 0.35)
      : mix(s.back, s.flank, Math.pow(x, 1.6));
    const light = 0.55 + diffuse * 0.75;
    colour = [colour[0] * light, colour[1] * light, colour[2] * light];
    if (diffuse > 0.62 && !onFin) colour = mix(colour, s.light, (diffuse - 0.62) * 1.6);
    if (
      s.spots &&
      !onFin &&
      hash(Math.floor(u * 14 + 7), Math.floor((d / swimmer.halfWidth) * 1.6 + 9)) < 0.16
    )
      colour = mix(colour, s.spots, 0.75);
    if (s.hammer && u < 0.07) colour = mix(s.back, s.flank, 0.4);
    const grain = (hash(col * 7 + (this.frame % 3), row) - 0.5) * 22;
    const [r, g, b] = colour.map((v) => clamp(v + grain, 0, 255)) as [number, number, number];
    const lum = (r * 0.3 + g * 0.59 + b * 0.11) / 255;
    return {
      col,
      row,
      fill: `rgb(${r | 0},${g | 0},${b | 0})`,
      alpha: onFin ? 0.55 - x * 0.3 : x > 0.82 ? 0.7 : 1,
      glyph: RAMP[clamp(Math.round((1 - lum) * (RAMP.length - 1)), 0, RAMP.length - 1)]!,
      ink:
        lum > 0.55
          ? `rgba(40,42,40,${0.55 + (1 - lum) * 0.3})`
          : `rgba(255,255,255,${0.5 + lum * 0.4})`,
    };
  }
}
