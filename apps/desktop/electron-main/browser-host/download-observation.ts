import type { BrowserDownloadObservation } from '@aura/contracts/generated';

const WINDOW_MS = 30_000;
const KEEP_MS = 45_000;

/** Electron gives an origin WebContents for a download, but no initiating click ID. */
export class DownloadObservation {
  #click: { id: string; until: number; ambiguous: boolean } | undefined;
  #receipt: { observation: BrowserDownloadObservation; until: number } | undefined;

  begin(id: string, now: number): void {
    const ambiguous = this.#click !== undefined && this.#click.until >= now;
    this.#click = { id, until: now + WINDOW_MS, ambiguous };
    this.#receipt = undefined;
  }

  interrupt(): void {
    this.#click = undefined;
    this.#receipt = undefined;
  }

  activeClick(now: number): string | undefined {
    return this.#click && this.#click.until >= now && !this.#click.ambiguous
      ? this.#click.id
      : undefined;
  }

  block(filename: string, now: number): void {
    const safeName = [...filename.slice(0, 255)]
      .map((character) => {
        const code = character.charCodeAt(0);
        return code < 32 || code === 127 ? ' ' : character;
      })
      .join('');
    this.#receipt = {
      observation: {
        status: 'blocked_by_host',
        click_id: this.activeClick(now) ?? null,
        filename: safeName,
      },
      until: now + KEEP_MS,
    };
  }

  result(
    now: number,
    clickId?: string,
    accessRequired = false,
  ): BrowserDownloadObservation | undefined {
    if (this.#receipt && this.#receipt.until >= now) return this.#receipt.observation;
    if (clickId !== undefined) {
      return { status: accessRequired ? 'access_required' : 'not_observed', click_id: clickId };
    }
    const pending = this.activeClick(now);
    return accessRequired && pending ? { status: 'access_required', click_id: pending } : undefined;
  }
}
