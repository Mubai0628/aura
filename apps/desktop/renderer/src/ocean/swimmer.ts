/**
 * A shark seen from above, swimming curves around the middle of the empty
 * conversation. Its body follows the path its head has traced, so it bends as it turns.
 */
export type Rgb = readonly [number, number, number];

export interface Species {
  /** Along the spine, out to the flanks, and the light catching it. */
  back: Rgb;
  flank: Rgb;
  light: Rgb;
  spots?: Rgb;
  /** Length and half-width as shares of the scene's scale; fins as multiples of the half-width. */
  length: number;
  width: number;
  fins: number;
  hammer?: boolean;
}

const rgb = (hex: string): Rgb => {
  const n = Number.parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
};

export const SPECIES = {
  white: {
    back: rgb('#4b5059'),
    flank: rgb('#9097a0'),
    light: rgb('#f1f2ef'),
    length: 0.42,
    width: 0.036,
    fins: 2.6,
  },
  blue: {
    back: rgb('#344d70'),
    flank: rgb('#7489a8'),
    light: rgb('#e4eaf1'),
    length: 0.4,
    width: 0.03,
    fins: 3,
  },
  leopard: {
    back: rgb('#8a7258'),
    flank: rgb('#c8b394'),
    light: rgb('#f4ece0'),
    spots: rgb('#33291f'),
    length: 0.36,
    width: 0.032,
    fins: 2.3,
  },
  hammer: {
    back: rgb('#5e635e'),
    flank: rgb('#9da199'),
    light: rgb('#eeeeea'),
    length: 0.38,
    width: 0.03,
    fins: 2.4,
    hammer: true,
  },
} satisfies Record<string, Species>;

export type SpeciesName = keyof typeof SPECIES;

/** The curve a swimmer keeps to: a slowly turning ellipse around the middle, with some wander. */
export interface Orbit {
  phase: number;
  rx: number;
  ry: number;
  /** Radians per second along the ellipse. */
  speed: number;
  /** Pixels per second through the water. */
  pace: number;
  wobble: number;
}

export interface SpinePoint {
  x: number;
  y: number;
  /** 0 at the snout, 1 at the base of the tail; the tail fin runs on past 1. */
  u: number;
}

const TAU = Math.PI * 2;
const clamp = (v: number, low = 0, high = 1) => Math.min(high, Math.max(low, v));
/** Rises to 1 at `peak`, back to 0 at 1; 0 outside [0, 1]. */
export const tri = (t: number, peak: number) =>
  t < 0 || t > 1 ? 0 : t < peak ? t / peak : (1 - t) / (1 - peak);

/** How far behind the head the body reaches, as a share of its length (the tail fin included). */
const REACH = 1.12;
/** How far apart the remembered head positions are, in pixels. */
const STEP = 3;

export class Swimmer {
  readonly species: Species;
  readonly length: number;
  readonly halfWidth: number;
  x = 0;
  y = 0;
  heading: number;
  /** Startled: swims faster and away until this runs out. */
  flee = 0;
  private wiggle: number;
  private trail: { x: number; y: number }[] = [];

  constructor(
    readonly kind: SpeciesName,
    scale: number,
    readonly orbit: Orbit,
    wiggle = 0,
  ) {
    this.species = SPECIES[kind];
    this.length = scale * this.species.length;
    this.halfWidth = scale * this.species.width;
    this.heading = orbit.phase + Math.PI / 2;
    this.wiggle = wiggle;
  }

  /** On its curve around (cx, cy), its body laid out straight behind it. */
  place(cx: number, cy: number): void {
    this.x = cx + Math.cos(this.orbit.phase) * this.orbit.rx;
    this.y = cy + Math.sin(this.orbit.phase) * this.orbit.ry;
    this.trail = [];
    const back = Math.ceil((this.length * REACH) / STEP) + 4;
    for (let i = 0; i < back; i++)
      this.trail.push({
        x: this.x - Math.cos(this.heading) * i * STEP,
        y: this.y - Math.sin(this.heading) * i * STEP,
      });
  }

  /** Swims for `dt` seconds toward its place on the curve, away from the pointer when it is close. */
  update(dt: number, cx: number, cy: number, pointer?: { x: number; y: number }): void {
    const o = this.orbit;
    o.phase += o.speed * dt;
    const tx = cx + Math.cos(o.phase) * o.rx + Math.sin(o.phase * 2.3 + o.wobble) * o.rx * 0.18;
    const ty = cy + Math.sin(o.phase) * o.ry + Math.cos(o.phase * 1.7 + o.wobble) * o.ry * 0.22;
    let want = Math.atan2(ty - this.y, tx - this.x);
    if (pointer && Math.hypot(pointer.x - this.x, pointer.y - this.y) < this.length * 0.55) {
      want = Math.atan2(this.y - pointer.y, this.x - pointer.x);
      this.flee = 1;
    }
    this.flee = Math.max(0, this.flee - dt * 0.6);
    const turn = ((want - this.heading + Math.PI * 3) % TAU) - Math.PI;
    this.heading += clamp(turn, -1.1 * dt, 1.1 * dt);
    this.wiggle += dt * (5 + this.flee * 6);
    const travel = (o.pace + this.flee * 90) * dt;
    this.x += Math.cos(this.heading) * travel;
    this.y += Math.sin(this.heading) * travel;
    const head = this.trail[0];
    if (!head || Math.hypot(this.x - head.x, this.y - head.y) >= STEP) {
      this.trail.unshift({ x: this.x, y: this.y });
      const keep = Math.ceil((this.length * REACH) / STEP) + 8;
      if (this.trail.length > keep) this.trail.length = keep;
    } else {
      head.x = this.x;
      head.y = this.y;
    }
  }

  /** Points from the snout along the traced path to the tail fin, with the body's sway. */
  spine(points = 34): SpinePoint[] {
    const result: SpinePoint[] = [];
    const total = this.length * REACH;
    let index = 0;
    let walked = 0;
    for (let k = 0; k <= points; k++) {
      const s = (k / points) * total;
      const u = s / this.length;
      let placed = false;
      while (index < this.trail.length - 1) {
        const a = this.trail[index]!;
        const b = this.trail[index + 1]!;
        const segment = Math.hypot(b.x - a.x, b.y - a.y) || 1e-6;
        if (walked + segment >= s) {
          const t = (s - walked) / segment;
          const nx = -(b.y - a.y) / segment;
          const ny = (b.x - a.x) / segment;
          const sway = Math.sin(this.wiggle - u * 5.5) * this.halfWidth * 0.55 * Math.pow(u, 1.6);
          result.push({
            x: a.x + (b.x - a.x) * t + nx * sway,
            y: a.y + (b.y - a.y) * t + ny * sway,
            u,
          });
          placed = true;
          break;
        }
        walked += segment;
        index += 1;
      }
      if (!placed) {
        const last = result.at(-1) ?? this.trail[0] ?? { x: this.x, y: this.y };
        result.push({ x: last.x, y: last.y, u });
      }
    }
    return result;
  }

  /** How far the body (and with its fins, the whole shark) reaches either side of the spine at u. */
  reach(u: number): { body: number; all: number } {
    const w = this.halfWidth;
    let body: number;
    if (u < 0.1) body = w * Math.sqrt(Math.max(u, 0) / 0.1) * 0.82;
    else if (u < 0.5) body = w * (0.82 + 0.18 * Math.sin(((u - 0.1) / 0.4) * Math.PI));
    else body = w * (0.82 - 0.66 * Math.pow((u - 0.5) / 0.48, 1.25));
    if (u > 0.98) body = 0;
    let fins = w * this.species.fins * tri((u - 0.24) / 0.2, 0.18);
    fins = Math.max(fins, w * 0.95 * tri((u - 0.6) / 0.1, 0.3));
    if (u > 0.9) fins = Math.max(fins, w * 1.05 * Math.sin(Math.PI * clamp((u - 0.9) / 0.22)));
    if (this.species.hammer && u < 0.07)
      fins = Math.max(fins, w * 2.5 * (1 - Math.abs(u - 0.035) / 0.035));
    body = Math.max(body, 0);
    return { body, all: Math.max(body, fins) };
  }
}
