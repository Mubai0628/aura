/**
 * For a page shown inside Aura's
 * window, Electron builds the detached DevTools page and never gives it a window. Each page's
 * DevTools get a window of their own instead: opened once, brought forward when asked again, and
 * closed with the page or with Aura's window.
 */
export interface DevToolsPage {
  readonly id: number;
  isDestroyed(): boolean;
  getTitle(): string;
  setDevToolsWebContents(contents: unknown): void;
  openDevTools(options: { mode: 'detach'; activate: boolean }): void;
  closeDevTools(): void;
  once(event: 'destroyed', listener: () => void): unknown;
}

export interface DevToolsHostWindow {
  readonly webContents: unknown;
  isDestroyed(): boolean;
  show(): void;
  focus(): void;
  close(): void;
  once(event: 'closed', listener: () => void): unknown;
}

export class DevToolsWindows {
  readonly #make: (title: string) => DevToolsHostWindow;
  /** The window showing each page's DevTools, by the page's web contents. */
  readonly #open = new Map<number, DevToolsHostWindow>();

  constructor(make: (title: string) => DevToolsHostWindow) {
    this.#make = make;
  }

  open(page: DevToolsPage, owner: DevToolsHostWindow | undefined): void {
    const shown = this.#open.get(page.id);
    if (shown !== undefined && !shown.isDestroyed()) {
      shown.show();
      shown.focus();
      return;
    }
    const window = this.#make(`开发者工具 · ${page.getTitle() || '页面'}`);
    this.#open.set(page.id, window);
    window.once('closed', () => {
      if (this.#open.get(page.id) === window) this.#open.delete(page.id);
      if (!page.isDestroyed()) page.closeDevTools();
    });
    const close = () => {
      if (!window.isDestroyed()) window.close();
    };
    page.once('destroyed', close);
    owner?.once('closed', close);
    page.setDevToolsWebContents(window.webContents);
    page.openDevTools({ mode: 'detach', activate: true });
    window.show();
    window.focus();
  }
}
