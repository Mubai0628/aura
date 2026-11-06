import type { BrowserWindow, Display, Rectangle } from 'electron';
import { lstat, mkdir, open, readdir, rename, unlink } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';

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

// `#write`'s temporary file: the saved file's name, the writing process id, then `.tmp`.
const TEMPORARY_NAME = /^window-state\.v1\.json\.(\d+)\.tmp$/u;
// A write holds its temporary file for milliseconds. A recent one named after a running process
// may be another Aura's write that is about to be renamed; any other was left by a process that
// ended between creating it and renaming it.
const IN_FLIGHT_MS = 60_000;

export class WindowStateStore {
  readonly #path: string;
  readonly #log: (event: string, fields: Record<string, unknown>) => void;
  #pending: Promise<void>;
  #closed = false;
  #failing = false;

  constructor(
    namespaceDataRoot: string,
    log: (event: string, fields: Record<string, unknown>) => void,
  ) {
    this.#path = resolve(namespaceDataRoot, 'desktop/window-state.v1.json');
    this.#log = log;
    this.#pending = this.#removeStaleTemporaryFiles();
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
    if (this.#closed || window.isMinimized()) return;
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
    this.#pending = this.#pending.then(() => this.#save(state));
  }

  async flush(): Promise<void> {
    await this.#pending;
  }

  /**
   * Quit waits here, and captures after it are ignored: the window's close event in Electron's
   * second quit pass would start a write that the exit cuts off before its rename.
   */
  async close(): Promise<void> {
    this.#closed = true;
    await this.#pending;
  }

  // A failed write must not stop the writes after it or fail quit. It is logged once until a write
  // succeeds again, so a window moved while the disk keeps failing does not flood the log.
  async #save(state: SavedWindowState): Promise<void> {
    try {
      await this.#write(state);
      this.#failing = false;
    } catch (error) {
      if (!this.#failing) {
        this.#log('window_state.write_failed', {
          error: error instanceof Error ? error.message : String(error),
        });
      }
      this.#failing = true;
    }
  }

  async #write(state: SavedWindowState): Promise<void> {
    await mkdir(dirname(this.#path), { recursive: true });
    const temporary = `${this.#path}.${String(process.pid)}.tmp`;
    try {
      const file = await open(temporary, 'w');
      try {
        await file.writeFile(JSON.stringify(state, null, 2), 'utf8');
        await file.sync();
      } finally {
        await file.close();
      }
      await rename(temporary, this.#path);
    } catch (error) {
      await unlink(temporary).catch(() => undefined);
      throw error;
    }
  }

  async #removeStaleTemporaryFiles(): Promise<void> {
    const directory = dirname(this.#path);
    let names: string[];
    try {
      names = await readdir(directory);
    } catch {
      return;
    }
    for (const name of names) {
      const pid = TEMPORARY_NAME.exec(name)?.[1];
      if (pid === undefined) continue;
      const path = join(directory, name);
      try {
        const stats = await lstat(path);
        if (!stats.isFile()) continue;
        if (Date.now() - stats.mtimeMs < IN_FLIGHT_MS && isRunning(Number(pid))) continue;
        await unlink(path);
      } catch {
        // Removed or held open by another process meanwhile; a later start tries again.
      }
    }
  }
}

const isRunning = (pid: number): boolean => {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    // EPERM means the process exists but this one may not signal it.
    return (error as NodeJS.ErrnoException).code !== 'ESRCH';
  }
};

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
