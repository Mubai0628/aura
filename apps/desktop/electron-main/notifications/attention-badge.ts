export interface AttentionBadgeDeps {
  /** Shows the dot for this many conversations, or takes it away at none. */
  apply(count: number): void;
  inFront(sessionId: string): boolean;
}

/**
 * The red dot on Aura's taskbar button: on while a conversation has a notification the user has
 * not looked at, gone once each such conversation has been in front.
 */
export class AttentionBadge {
  readonly #unseen = new Set<string>();
  readonly #deps: AttentionBadgeDeps;

  constructor(deps: AttentionBadgeDeps) {
    this.#deps = deps;
  }

  get count(): number {
    return this.#unseen.size;
  }

  add(sessionId: string): void {
    if (this.#unseen.has(sessionId)) return;
    this.#unseen.add(sessionId);
    this.#deps.apply(this.#unseen.size);
  }

  /** Forgets the conversations in front now. */
  look(): void {
    let changed = false;
    for (const sessionId of [...this.#unseen])
      if (this.#deps.inFront(sessionId)) {
        this.#unseen.delete(sessionId);
        changed = true;
      }
    if (changed) this.#deps.apply(this.#unseen.size);
  }
}

/** The dot's side in pixels; Windows scales an overlay to its own size. */
export const RED_DOT_SIZE = 16;

/** A red dot with a white rim, as BGRA pixels, so no image file ships for it. */
export function redDotPixels(): Buffer {
  const pixels = Buffer.alloc(RED_DOT_SIZE * RED_DOT_SIZE * 4);
  const center = (RED_DOT_SIZE - 1) / 2;
  for (let y = 0; y < RED_DOT_SIZE; y += 1)
    for (let x = 0; x < RED_DOT_SIZE; x += 1) {
      const distance = Math.hypot(x - center, y - center);
      if (distance > 7.5) continue;
      const red = distance <= 6;
      const at = (y * RED_DOT_SIZE + x) * 4;
      pixels[at] = red ? 0x4d : 0xff;
      pixels[at + 1] = red ? 0x48 : 0xff;
      pixels[at + 2] = red ? 0xe5 : 0xff;
      pixels[at + 3] = 0xff;
    }
  return pixels;
}
