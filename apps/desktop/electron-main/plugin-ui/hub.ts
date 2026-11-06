/**
 * Every plugin's notices and status entries, from wherever they come (its panels, its background
 * service), once Core checked them. A plugin shows at most three notices in ten seconds. Its
 * status entry changes on screen at most once a second and the last value stays, so the status
 * bar never flickers; an entry belongs to what set it and goes when that goes.
 */
import {
  PLUGIN_STATUS_ENTRIES_MAX,
  type PluginNoticeShown,
  type PluginStatusEntry,
  type PluginUiChange,
  type PluginUiTone,
} from '../../shared/plugin-ui-api.js';

export const NOTICES_PER_WINDOW = 3;
export const NOTICE_WINDOW_MS = 10_000;
export const STATUS_QUIET_MS = 1_000;

/** Time for the hub; tests pass their own. */
export interface HubClock {
  now(): number;
  /** Runs `run` after `ms`; returns the cancel. */
  later(run: () => void, ms: number): () => void;
}

const REAL_CLOCK: HubClock = {
  now: () => Date.now(),
  later: (run, ms) => {
    const id = setTimeout(run, ms);
    return () => clearTimeout(id);
  },
};

interface Held {
  text: string;
  tone: PluginUiTone;
  /** Which set came last, across the plugin's sources. */
  order: number;
}

const same = (a: PluginStatusEntry | undefined, b: PluginStatusEntry | undefined) =>
  a?.text === b?.text && a?.tone === b?.tone;

export class PluginUiHub {
  readonly #emit: (change: PluginUiChange) => void;
  readonly #clock: HubClock;
  readonly #notices = new Map<string, number[]>();
  /** What each source holds, by plugin. */
  readonly #held = new Map<string, Map<string, Held>>();
  /** What the window shows, by plugin, and when that last changed. */
  readonly #shown = new Map<string, PluginStatusEntry>();
  readonly #changedAt = new Map<string, number>();
  readonly #waiting = new Map<string, () => void>();
  #order = 0;

  constructor(emit: (change: PluginUiChange) => void, clock: HubClock = REAL_CLOCK) {
    this.#emit = emit;
    this.#clock = clock;
  }

  /** Whether the plugin may show another notice now. */
  noticeAllowed(plugin: string): boolean {
    return this.#recent(plugin).length < NOTICES_PER_WINDOW;
  }

  /** Shows a notice Core checked; false when the plugin showed too many just now. */
  notice(notice: PluginNoticeShown): boolean {
    const recent = this.#recent(notice.plugin);
    if (recent.length >= NOTICES_PER_WINDOW) return false;
    recent.push(this.#clock.now());
    this.#emit({ kind: 'notice', notice });
    return true;
  }

  /** Sets the entry `source` holds for the plugin; without text it is cleared. */
  status(plugin: string, source: string, text: string | undefined, tone: PluginUiTone): void {
    const held = this.#held.get(plugin) ?? new Map<string, Held>();
    if (text === undefined) held.delete(source);
    else held.set(source, { text, tone, order: ++this.#order });
    if (held.size === 0) this.#held.delete(plugin);
    else this.#held.set(plugin, held);
    this.#settle(plugin);
  }

  /** What `source` held for the plugin goes with it: a panel closed, a service stopped. */
  drop(plugin: string, source: string): void {
    const held = this.#held.get(plugin);
    if (held === undefined || !held.delete(source)) return;
    if (held.size === 0) this.#held.delete(plugin);
    this.#settle(plugin);
  }

  /** What `source` held for every plugin goes: the Core that ran the services is gone. */
  dropSource(source: string): void {
    for (const plugin of [...this.#held.keys()]) this.drop(plugin, source);
  }

  /** The plugin's background service changed state; the plugin page reads it again. */
  serviceChanged(plugin: string): void {
    this.#emit({ kind: 'service', plugin });
  }

  /** The entries shown now, one per plugin. */
  statuses(): PluginStatusEntry[] {
    return [...this.#shown.values()].slice(0, PLUGIN_STATUS_ENTRIES_MAX);
  }

  #recent(plugin: string): number[] {
    const now = this.#clock.now();
    const recent = (this.#notices.get(plugin) ?? []).filter((at) => now - at < NOTICE_WINDOW_MS);
    this.#notices.set(plugin, recent);
    return recent;
  }

  /** The newest entry any of the plugin's sources holds. */
  #wanted(plugin: string): PluginStatusEntry | undefined {
    let newest: Held | undefined;
    for (const held of this.#held.get(plugin)?.values() ?? [])
      if (newest === undefined || held.order > newest.order) newest = held;
    return newest === undefined ? undefined : { plugin, text: newest.text, tone: newest.tone };
  }

  /** Brings what the window shows for the plugin to what is held, at most once a second. */
  #settle(plugin: string): void {
    // A change already waiting takes whatever is held when its time comes.
    if (this.#waiting.has(plugin)) return;
    const wanted = this.#wanted(plugin);
    if (same(wanted, this.#shown.get(plugin))) return;
    const wait =
      (this.#changedAt.get(plugin) ?? Number.NEGATIVE_INFINITY) +
      STATUS_QUIET_MS -
      this.#clock.now();
    if (wait > 0) {
      const cancel = this.#clock.later(() => {
        this.#waiting.delete(plugin);
        this.#settle(plugin);
      }, wait);
      this.#waiting.set(plugin, cancel);
      return;
    }
    if (wanted === undefined) this.#shown.delete(plugin);
    else this.#shown.set(plugin, wanted);
    this.#changedAt.set(plugin, this.#clock.now());
    this.#emit({ kind: 'statuses', statuses: this.statuses() });
  }
}
