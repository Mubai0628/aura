/**
 * A page of Aura's browser read for a message that refers to it: its title, address and the text
 * it shows. The text is read in a world of its own, apart from the page's scripts, so the page
 * cannot change how it is read.
 */
import { PAGE_TEXT_CHARS, type PageText } from '../../shared/browser-api.js';

export { PAGE_TEXT_CHARS } from '../../shared/browser-api.js';

/** The parts of Electron's WebContents the reading uses; tests pass a fake. */
export interface TextContents {
  isDestroyed(): boolean;
  getTitle(): string;
  getURL(): string;
  executeJavaScriptInIsolatedWorld(worldId: number, scripts: { code: string }[]): Promise<unknown>;
}

/** The world the reading runs in: neither the page's own (0) nor Electron's isolation world (999). */
const WORLD = 1_016;
/** How long the reading may take; a page that does not answer by then is not read. */
export const READ_LIMIT_MS = 3_000;
const TIMED_OUT = Symbol('timed out');

export async function pageText(contents: TextContents | undefined): Promise<PageText | undefined> {
  if (contents === undefined || contents.isDestroyed()) return undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const read = await Promise.race([
      contents.executeJavaScriptInIsolatedWorld(WORLD, [
        { code: 'document.body ? document.body.innerText : ""' },
      ]),
      new Promise<symbol>((done) => {
        timer = setTimeout(() => done(TIMED_OUT), READ_LIMIT_MS);
      }),
    ]);
    if (read === TIMED_OUT) return undefined;
    const text = typeof read === 'string' ? read : '';
    return {
      title: contents.getTitle(),
      url: contents.getURL(),
      text: text.slice(0, PAGE_TEXT_CHARS),
      truncated: text.length > PAGE_TEXT_CHARS,
    };
  } catch {
    return undefined;
  } finally {
    clearTimeout(timer);
  }
}
