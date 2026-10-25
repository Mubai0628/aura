import { useSyncExternalStore } from 'react';

/**
 * The right sidebar holds tabs. 终端、文件 open as many times as asked;
 * 审查、进程、浏览器、侧边对话、提示词监测 open once, and once open they leave the "+" menu. A terminal the model opened
 * becomes a tab only when the user asks to see it. Each conversation keeps its own tabs while Aura
 * runs.
 */
export type ToolKind =
  'review' | 'terminal' | 'process' | 'browser' | 'files' | 'chat' | 'prompt-inspector';

export const TOOL_KINDS: readonly ToolKind[] = [
  'review',
  'terminal',
  'process',
  'browser',
  'files',
  'chat',
  'prompt-inspector',
];

/**
 * What "+" and the empty sidebar offer.
 * 提示词监测 still opens from the context meter, `/context` and a step's "检查此步骤".
 */
export const MENU_TOOLS: readonly ToolKind[] = TOOL_KINDS.filter(
  (kind) => kind !== 'prompt-inspector',
);

/** The tools that may be open more than once. */
export const MULTIPLE: ReadonlySet<ToolKind> = new Set(['terminal', 'files']);

export const TOOL_NAMES: Record<ToolKind, string> = {
  review: '审查',
  terminal: '终端',
  process: '进程',
  browser: '浏览器',
  files: '文件',
  chat: '侧边对话',
  'prompt-inspector': '提示词监测',
};

export interface ToolTab {
  id: string;
  kind: ToolKind;
  /** A tool opened from "+": its number among the tabs of its kind in this conversation. */
  number?: number;
  /** A terminal tab's terminal, once it is open; one opened elsewhere names it from the start. */
  terminalId?: string;
  /** A 文件 tab's open files (workspace-relative), each a small tab of the page. */
  files?: string[];
  /** The one of them shown, if any. */
  file?: string | undefined;
  /** The line asked for, and how many times it was asked: each request shows it again. */
  line?: number | undefined;
  shown?: number;
}

export interface SessionTabs {
  tabs: ToolTab[];
  active?: string | undefined;
}

const EMPTY: SessionTabs = { tabs: [] };

class ToolTabStore {
  readonly #sessions = new Map<string, SessionTabs>();
  readonly #numbers = new Map<string, number>();
  readonly #listeners = new Set<() => void>();
  readonly #revealListeners = new Set<(session: string) => void>();
  #ids = 0;

  get = (session: string): SessionTabs => this.#sessions.get(session) ?? EMPTY;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  /** Opens a tool: one that opens once shows its tab again; any other gets a new tab. */
  open(session: string, kind: ToolKind): string {
    const current = this.get(session);
    if (!MULTIPLE.has(kind)) {
      const open = current.tabs.find((tab) => tab.kind === kind);
      if (open) return this.activate(session, open.id);
    }
    const key = `${session}:${kind}`;
    const number = (this.#numbers.get(key) ?? 0) + 1;
    this.#numbers.set(key, number);
    const tab: ToolTab = { id: `tab-${++this.#ids}`, kind, number };
    this.#put(session, { tabs: [...current.tabs, tab], active: tab.id });
    return tab.id;
  }

  /** Shows a tool: the latest tab of its kind, or a new one (shortcuts and links). */
  show(session: string, kind: ToolKind): string {
    const latest = this.get(session).tabs.findLast((tab) => tab.kind === kind);
    return latest ? this.activate(session, latest.id) : this.open(session, kind);
  }

  /** A terminal opened elsewhere (the model's): its tab, made once and bound to it. */
  showTerminal(session: string, terminalId: string): string {
    const current = this.get(session);
    const open = current.tabs.find((tab) => tab.terminalId === terminalId);
    if (open) return this.activate(session, open.id);
    const tab: ToolTab = { id: `tab-${++this.#ids}`, kind: 'terminal', terminalId };
    this.#put(session, { tabs: [...current.tabs, tab], active: tab.id });
    return tab.id;
  }

  /** A tool asked for elsewhere in the window: its latest tab, shown, and the sidebar. */
  reveal(session: string, kind: ToolKind): void {
    this.show(session, kind);
    for (const listener of this.#revealListeners) listener(session);
  }

  /** A link elsewhere in the window (the conversation) shows a terminal: its tab, and the sidebar. */
  revealTerminal(session: string, terminalId: string): void {
    this.showTerminal(session, terminalId);
    for (const listener of this.#revealListeners) listener(session);
  }

  /** The window opens its right sidebar when a link asks to show a tab there. */
  onReveal = (listener: (session: string) => void): (() => void) => {
    this.#revealListeners.add(listener);
    return () => {
      this.#revealListeners.delete(listener);
    };
  };

  /**
   * Opens a workspace file in a 文件 page: the page on
   * screen, else the last 文件 page opened, else a new one; the file joins that page's small tabs
   * once and is shown. A line, when given, is shown again even if it was asked for before.
   */
  openFile(session: string, path: string, line?: number): string {
    const current = this.get(session);
    const pages = current.tabs.filter((tab) => tab.kind === 'files');
    const target = pages.find((tab) => tab.id === current.active) ?? pages.at(-1);
    if (target) {
      this.#put(session, {
        tabs: current.tabs.map((tab) =>
          tab.id === target.id
            ? {
                ...tab,
                files: tab.files?.includes(path) ? tab.files : [...(tab.files ?? []), path],
                file: path,
                line,
                shown: (tab.shown ?? 0) + 1,
              }
            : tab,
        ),
        active: target.id,
      });
      return target.id;
    }
    const key = `${session}:files`;
    const number = (this.#numbers.get(key) ?? 0) + 1;
    this.#numbers.set(key, number);
    const tab: ToolTab = {
      id: `tab-${++this.#ids}`,
      kind: 'files',
      number,
      files: [path],
      file: path,
      line,
      shown: 1,
    };
    this.#put(session, { tabs: [...current.tabs, tab], active: tab.id });
    return tab.id;
  }

  /** Shows one of a 文件 page's open files. */
  showFile(session: string, id: string, path: string): void {
    this.#editPage(session, id, (tab) =>
      tab.files?.includes(path) && tab.file !== path
        ? { ...tab, file: path, line: undefined }
        : tab,
    );
  }

  /** Closes one of a 文件 page's files; its neighbour (the next, else the previous) is shown. */
  closeFile(session: string, id: string, path: string): void {
    this.#editPage(session, id, (tab) => {
      const files = tab.files ?? [];
      const index = files.indexOf(path);
      if (index < 0) return tab;
      const rest = files.filter((file) => file !== path);
      const file = tab.file === path ? (rest[index] ?? rest[index - 1]) : tab.file;
      return { ...tab, files: rest, file, line: file === tab.file ? tab.line : undefined };
    });
  }

  #editPage(session: string, id: string, edit: (tab: ToolTab) => ToolTab): void {
    const current = this.get(session);
    const tabs = current.tabs.map((tab) =>
      tab.id === id && tab.kind === 'files' ? edit(tab) : tab,
    );
    if (tabs.some((tab, index) => tab !== current.tabs[index]))
      this.#put(session, { ...current, tabs });
  }

  /** A link in the conversation shows a file: its tab, and the sidebar. */
  revealFile(session: string, path: string, line?: number): void {
    this.openFile(session, path, line);
    for (const listener of this.#revealListeners) listener(session);
  }

  /** A running terminal of the user's own with no tab (after a reload) gets one back, not shown. */
  adoptTerminal(session: string, terminalId: string): void {
    const current = this.get(session);
    if (current.tabs.some((tab) => tab.terminalId === terminalId)) return;
    const key = `${session}:terminal`;
    const number = (this.#numbers.get(key) ?? 0) + 1;
    this.#numbers.set(key, number);
    const tab: ToolTab = { id: `tab-${++this.#ids}`, kind: 'terminal', number, terminalId };
    this.#put(session, { tabs: [...current.tabs, tab], active: current.active ?? tab.id });
  }

  bindTerminal(session: string, id: string, terminalId: string): void {
    const current = this.get(session);
    this.#put(session, {
      ...current,
      tabs: current.tabs.map((tab) => (tab.id === id ? { ...tab, terminalId } : tab)),
    });
  }

  activate(session: string, id: string): string {
    const current = this.get(session);
    if (current.active !== id && current.tabs.some((tab) => tab.id === id))
      this.#put(session, { ...current, active: id });
    return id;
  }

  /** Closes a tab; the open tab's neighbour (the next, else the previous) is shown instead. */
  close(session: string, id: string): void {
    const current = this.get(session);
    const index = current.tabs.findIndex((tab) => tab.id === id);
    if (index < 0) return;
    const tabs = current.tabs.filter((tab) => tab.id !== id);
    const active = current.active === id ? (tabs[index] ?? tabs[index - 1])?.id : current.active;
    this.#put(session, { tabs, active });
  }

  resetForTests(): void {
    this.#sessions.clear();
    this.#numbers.clear();
    this.#ids = 0;
  }

  #put(session: string, value: SessionTabs): void {
    this.#sessions.set(session, value);
    for (const listener of this.#listeners) listener();
  }
}

export const toolTabs = new ToolTabStore();

/** The tools "+" offers: every tool that may open more than once, and the others not yet open. */
export function availableTools(state: SessionTabs): ToolKind[] {
  return MENU_TOOLS.filter(
    (kind) => MULTIPLE.has(kind) || !state.tabs.some((tab) => tab.kind === kind),
  );
}

/**
 * Each tab with its label: terminals are numbered; 文件 pages when more than one is open. A 文件
 * page keeps its name whatever files are open in it.
 */
export function tabLabels(state: SessionTabs): { tab: ToolTab; label: string }[] {
  return state.tabs.map((tab) => {
    const name = TOOL_NAMES[tab.kind];
    const several = state.tabs.filter((other) => other.kind === tab.kind).length > 1;
    const numbered = tab.number !== undefined && (tab.kind === 'terminal' || several);
    return { tab, label: numbered ? `${name} ${tab.number}` : name };
  });
}

export function useToolTabs(session: string | undefined): SessionTabs {
  return useSyncExternalStore(toolTabs.subscribe, () =>
    session === undefined ? EMPTY : toolTabs.get(session),
  );
}

export function resetToolTabsForTests(): void {
  toolTabs.resetForTests();
}
