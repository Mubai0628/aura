/**
 * The model's page never takes the keyboard. Loading
 * or showing a page can give its view the focus; the keys the user was typing into the message box
 * then went to the page, and counted as the user taking the page over. Unless the user pressed on
 * the page themselves, the focus goes straight back to Aura's window, where the caret was. The
 * model needs no focus: its input reaches the page through DevTools, which emulates focus.
 */

/** How long after the user pressed on the page a focus change is theirs. */
export const PRESSED_MS = 1500;
/** How long a focus change is given for the press that caused it to arrive. */
export const SETTLE_MS = 80;

export interface FocusSource {
  on(event: 'focus', listener: () => void): unknown;
  isFocused(): boolean;
  isDestroyed(): boolean;
}

/** When the user last pressed on the page with a mouse or a finger. */
export class PagePresses {
  #last = Number.NEGATIVE_INFINITY;

  pressed(now = Date.now()): void {
    this.#last = now;
  }

  recent(now = Date.now()): boolean {
    return now - this.#last < PRESSED_MS;
  }
}

export function keepKeyboardForUser(
  contents: FocusSource,
  presses: PagePresses,
  giveBack: () => void,
  schedule: (run: () => void, ms: number) => unknown = setTimeout,
): void {
  contents.on('focus', () => {
    schedule(() => {
      if (presses.recent() || contents.isDestroyed() || !contents.isFocused()) return;
      giveBack();
    }, SETTLE_MS);
  });
}
