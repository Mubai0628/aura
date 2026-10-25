import type { BrowserWindow, Display, Rectangle } from 'electron';
import { mkdir, open, rename } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

export type SavedWindowMode = 'fullscreen' | 'maximized' | 'normal';

export interface SavedWindowState {
  display_id: string;
  normal_bounds_dip: Rectangle;
  saved_scale_factor: number;
  schema_version: 1;
  window_state: SavedWindowMode;
}

export interface RestoredWindowState {
  bounds: Rectangle;
  mode: SavedWindowMode;
}

const DEFAULT_BOUNDS: Rectangle = { height: 720, width: 1100, x: 120, y: 80 };

export const restoreWindowState = (
  saved: unknown,
  displays: readonly Display[],
  primaryDisplay: Display,
): RestoredWindowState => {
  const state = parseSavedState(saved);
  if (state === undefined) {
    return { bounds: centerInWorkArea(DEFAULT_BOUNDS, primaryDisplay.workArea), mode: 'normal' };
  }
  const display =
    displays.find((candidate) => String(candidate.id) === state.display_id) ?? primaryDisplay;
  return {
    bounds: clampToWorkArea(state.normal_bounds_dip, display.workArea),
    mode: state.window_state,
  };
};

export class WindowStateStore {
  readonly #path: string;
  #pending: Promise<void> = Promise.resolve();

  constructor(namespaceDataRoot: string) {
    this.#path = resolve(namespaceDataRoot, 'desktop/window-state.v1.json');
  }

  async read(): Promise<unknown> {
    try {
      const file = await open(this.#path, 'r');
      try {
        return JSON.parse(await file.readFile({ encoding: 'utf8' })) as unknown;
      } finally {
        await file.close();
      }
    } catch {
      return undefined;
    }
  }

  capture(window: BrowserWindow, display: Display): void {
    if (window.isMinimized()) return;
    const state: SavedWindowState = {
      display_id: String(display.id),
      normal_bounds_dip: window.getNormalBounds(),
      saved_scale_factor: display.scaleFactor,
      schema_version: 1,
      window_state: window.isFullScreen()
        ? 'fullscreen'
        : window.isMaximized()
          ? 'maximized'
          : 'normal',
    };
    this.#pending = this.#pending.then(() => this.#write(state));
  }

  async flush(): Promise<void> {
    await this.#pending;
  }

  async #write(state: SavedWindowState): Promise<void> {
    await mkdir(dirname(this.#path), { recursive: true });
    const temporary = `${this.#path}.${String(process.pid)}.tmp`;
    const file = await open(temporary, 'w');
    try {
      await file.writeFile(JSON.stringify(state, null, 2), 'utf8');
      await file.sync();
    } finally {
      await file.close();
    }
    await rename(temporary, this.#path);
  }
}

const parseSavedState = (value: unknown): SavedWindowState | undefined => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return undefined;
  const state = value as Partial<SavedWindowState>;
  if (
    state.schema_version !== 1 ||
    typeof state.display_id !== 'string' ||
    typeof state.saved_scale_factor !== 'number' ||
    !['normal', 'maximized', 'fullscreen'].includes(state.window_state ?? '') ||
    !isRectangle(state.normal_bounds_dip)
  ) {
    return undefined;
  }
  return state as SavedWindowState;
};

const isRectangle = (value: unknown): value is Rectangle => {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) return false;
  const bounds = value as Partial<Rectangle>;
  return [bounds.x, bounds.y, bounds.width, bounds.height].every(
    (item) => typeof item === 'number' && Number.isFinite(item),
  );
};

const clampToWorkArea = (bounds: Rectangle, workArea: Rectangle): Rectangle => {
  const width = Math.min(Math.max(Math.round(bounds.width), 800), workArea.width);
  const height = Math.min(Math.max(Math.round(bounds.height), 540), workArea.height);
  const maximumX = workArea.x + workArea.width - width;
  const maximumY = workArea.y + workArea.height - Math.min(height, 48);
  return {
    height,
    width,
    x: Math.min(Math.max(Math.round(bounds.x), workArea.x), maximumX),
    y: Math.min(Math.max(Math.round(bounds.y), workArea.y), maximumY),
  };
};

const centerInWorkArea = (bounds: Rectangle, workArea: Rectangle): Rectangle =>
  clampToWorkArea(
    {
      ...bounds,
      x: workArea.x + Math.round((workArea.width - bounds.width) / 2),
      y: workArea.y + Math.round((workArea.height - bounds.height) / 2),
    },
    workArea,
  );
