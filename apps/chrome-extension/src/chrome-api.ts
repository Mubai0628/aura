/**
 * The parts of Chrome's extension API this extension uses. Only the service worker and the side
 * panel's entry read the global `chrome`; every other module takes what it needs as an argument,
 * so its tests pass small fakes.
 */
export interface ChromeEvent<Listener> {
  addListener(listener: Listener): void;
  removeListener(listener: Listener): void;
}

export interface Port {
  name: string;
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: ChromeEvent<(message: unknown) => void>;
  onDisconnect: ChromeEvent<() => void>;
}

export interface Tab {
  id?: number;
  windowId: number;
  url?: string;
  pendingUrl?: string;
  title?: string;
  active: boolean;
  groupId: number;
  status?: string;
}

export interface Debuggee {
  tabId?: number;
}

export interface StorageArea {
  get(key: string): Promise<Record<string, unknown>>;
  set(items: Record<string, unknown>): Promise<void>;
  remove(key: string): Promise<void>;
}

export interface DebuggerApi {
  attach(target: { tabId: number }, version: string): Promise<void>;
  detach(target: { tabId: number }): Promise<void>;
  sendCommand(target: { tabId: number }, method: string, params?: object): Promise<unknown>;
  onEvent: ChromeEvent<(source: Debuggee, method: string, params?: object) => void>;
  onDetach: ChromeEvent<(source: Debuggee, reason: string) => void>;
}

export interface TabsApi {
  get(tabId: number): Promise<Tab>;
  query(info: {
    active?: boolean;
    currentWindow?: boolean;
    lastFocusedWindow?: boolean;
  }): Promise<Tab[]>;
  create(properties: { url: string; active?: boolean; windowId?: number }): Promise<Tab>;
  remove(tabId: number): Promise<void>;
  update(tabId: number, properties: { active?: boolean }): Promise<Tab>;
  group(options: { tabIds: number[]; groupId?: number }): Promise<number>;
  ungroup(tabIds: number[]): Promise<void>;
  onUpdated: ChromeEvent<
    (tabId: number, change: { url?: string; title?: string; status?: string }, tab: Tab) => void
  >;
  onRemoved: ChromeEvent<(tabId: number) => void>;
  onActivated: ChromeEvent<(info: { tabId: number; windowId: number }) => void>;
}

export interface WindowsApi {
  get(windowId: number): Promise<{ state?: string }>;
}

export interface TabGroupsApi {
  query(info: { title?: string; windowId?: number }): Promise<{ id: number; windowId: number }[]>;
  update(groupId: number, properties: { title?: string; color?: string }): Promise<unknown>;
}

export interface ChromeApi {
  runtime: {
    id: string;
    lastError?: { message?: string };
    getManifest(): { version: string };
    connectNative(application: string): Port;
    connect(info: { name: string }): Port;
    onConnect: ChromeEvent<(port: Port) => void>;
  };
  storage: { local: StorageArea };
  debugger: DebuggerApi;
  tabs: TabsApi;
  windows: WindowsApi;
  tabGroups: TabGroupsApi;
  sidePanel: { setPanelBehavior(behavior: { openPanelOnActionClick: boolean }): Promise<void> };
}

export function chromeApi(): ChromeApi {
  return (globalThis as unknown as { chrome: ChromeApi }).chrome;
}
