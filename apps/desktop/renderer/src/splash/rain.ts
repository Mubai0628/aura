/**
 * The water under the startup screen. Drops fall onto a lake seen at a slant; each stirs
 * a ring that spreads, meets the others and fades, and while Core starts the rain keeps falling.
 */

/** How much the lake is squashed: rings far up the surface look flatter. */
export const SLANT = 0.55;
/**
 * A ring's crests: wide enough to span a row of characters once squashed, strong and slow enough
 * to read as a ring while it spreads (the integrated screen, measured against the preview, showed
 * the narrower rings only as stray dashes).
 */
const WAVENUMBER = 0.16;
const STRENGTH = 1.6;
const FADE = 0.7;
/** Seconds a ring lives on the water. */
export const RING_SECONDS = 3.2;
/** The rain thickens from this many drops a second to `RAIN_FULL` over the first `RAIN_RISE` s. */
const RAIN_START = 3;
const RAIN_FULL = 10;
const RAIN_RISE = 1.1;

export interface View {
  w: number;
  h: number;
}

export interface Drop {
  x: number;
  y: number;
  /** When it met the water, in seconds since the window opened. */
  at: number;
  size: number;
}

export interface FallingDrop extends Drop {
  /** When it meets the water; until then it is seen falling. */
  land: number;
}

const clamp = (v: number, low = 0, high = 1) => Math.min(high, Math.max(low, v));

/** A repeatable sequence in [0, 1), so the same start always rains the same way. */
export function seeded(seed = 1): () => number {
  let state = seed;
  return () => {
    state = (state * 16807) % 2147483647;
    return (state - 1) / 2147483646;
  };
}

/** How near the bottom of the view a point is, as the depth that widens rings there. */
export const depthAt = (y: number, view: View) => 0.55 + 0.45 * (y / view.h);

/** The water's height at (x, y) at `t` seconds: long slow swells and every drop's spreading rings. */
export function surface(
  x: number,
  y: number,
  t: number,
  drops: readonly Drop[],
  view: View,
): number {
  const depth = depthAt(y, view);
  const speed = Math.min(view.w, view.h) * 0.16;
  let h =
    0.16 * Math.sin(x * 0.018 + t * 0.9 + Math.sin(y * 0.05) * 1.4) * Math.sin(y * 0.09 - t * 0.6);
  for (const drop of drops) {
    const age = t - drop.at;
    if (age < 0) continue;
    const r = Math.hypot(x - drop.x, (y - drop.y) / SLANT) / depth;
    const front = speed * age;
    const behind = front - r;
    if (behind < -10 || behind > front * 0.9 + 26) continue;
    // A ring with smaller rings trailing it, all fading as they spread.
    const wave =
      Math.cos(behind * WAVENUMBER) *
      Math.exp(-behind * 0.035) *
      Math.exp(-age * FADE) *
      drop.size *
      STRENGTH;
    h += wave * (behind < 0 ? Math.exp(behind * 0.5) : 1);
  }
  return h;
}

/** The drops in the air and the rings on the water. */
export class Rain {
  readonly drops: Drop[] = [];
  readonly falling: FallingDrop[] = [];
  private last = 0;

  constructor(private readonly random: () => number) {}

  /** Rains on until `t`; a screen that is leaving lets no new drop land. */
  update(t: number, view: View, leaving: boolean): void {
    const rate = RAIN_START + (RAIN_FULL - RAIN_START) * clamp(t / RAIN_RISE);
    if (!leaving) for (let n = Math.floor((t - this.last) * rate); n > 0; n--) this.spawn(t, view);
    if (t - this.last >= 1 / rate) this.last = t;
    if (!leaving)
      for (let i = this.falling.length - 1; i >= 0; i--) {
        const drop = this.falling[i]!;
        if (t < drop.land) continue;
        this.drops.push({ x: drop.x, y: drop.y, at: drop.land, size: drop.size });
        this.falling.splice(i, 1);
      }
    for (let i = this.drops.length - 1; i >= 0; i--)
      if (t - this.drops[i]!.at > RING_SECONDS) this.drops.splice(i, 1);
  }

  /** Rings already spreading at `t`, for a picture that does not move. */
  scatter(t: number, view: View, count = 14): void {
    this.drops.length = 0;
    this.falling.length = 0;
    for (let i = 0; i < count; i++)
      this.drops.push({
        x: (0.08 + this.random() * 0.84) * view.w,
        y: (0.14 + this.random() * 0.76) * view.h,
        at: t - this.random() * 2.4,
        size: 0.6 + this.random() * 0.8,
      });
  }

  private spawn(t: number, view: View): void {
    this.falling.push({
      x: (0.06 + this.random() * 0.88) * view.w,
      y: (0.12 + this.random() * 0.8) * view.h,
      at: t,
      land: t + 0.22 + this.random() * 0.1,
      size: 0.6 + this.random() * 0.8,
    });
  }
}
