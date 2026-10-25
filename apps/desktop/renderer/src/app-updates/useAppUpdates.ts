import type { AppUpdateCommand, AppUpdateState } from '../../../shared/app-updates';
import { useSyncExternalStore } from 'react';

export interface AppUpdateController {
  state: AppUpdateState | undefined;
  error: string | undefined;
  pending: AppUpdateCommand['command'] | undefined;
  send: (command: AppUpdateCommand) => Promise<boolean>;
}

type Bridge = Pick<Window['aura']['v1'], 'appUpdate' | 'onAppUpdateState'>;
interface Request {
  command: AppUpdateCommand;
  generation: number;
  push: number;
  sequence: number;
}

const ERRORS: Record<AppUpdateCommand['command'], string> = {
  state: '无法读取更新状态，请重试。',
  check: '检查更新失败，请重试。',
  download: '下载更新失败，请重试。',
  pause: '暂停下载失败，请重试。',
  restart: '重启未完成，请重试。',
  roll_back: '回退未完成，请重试。',
  dismiss_notice: '更新提示未关闭，请重试。',
  set_auto_check: '自动检查设置未保存，请重试。',
};
const unavailableReply = (state: AppUpdateState) => !state.available && state.current === '';
const phaseReportsFailure = new Set<AppUpdateCommand['command']>([
  'check',
  'download',
  'roll_back',
]);

class Updates {
  #listeners = new Set<() => void>();
  #unsubscribe: (() => void) | undefined;
  #generation = 0;
  #push = 0;
  #sequence = 0;
  #active: Request | undefined;
  #pause: Request | undefined;
  #snapshot: AppUpdateController = {
    state: undefined,
    error: undefined,
    pending: undefined,
    send: (command) => this.send(command),
  };

  constructor(private readonly bridge: Bridge) {}

  snapshot = () => this.#snapshot;

  #publish(change: Partial<Omit<AppUpdateController, 'send'>>): void {
    this.#snapshot = { ...this.#snapshot, ...change };
    for (const listener of this.#listeners) listener();
  }

  #request(command: AppUpdateCommand): Request {
    return { command, generation: this.#generation, push: this.#push, sequence: ++this.#sequence };
  }

  #mounted(request: Request): boolean {
    return request.generation === this.#generation && this.#listeners.size > 0;
  }

  #current(request: Request): boolean {
    return this.#latest(request) && request.push === this.#push;
  }

  #latest(request: Request): boolean {
    return this.#mounted(request) && request.sequence === this.#sequence;
  }

  #listen(): boolean {
    const generation = this.#generation;
    try {
      this.#unsubscribe = this.bridge.onAppUpdateState((state) => {
        if (generation !== this.#generation || this.#listeners.size === 0) return;
        this.#push++;
        this.#publish(
          unavailableReply(state) ? { error: ERRORS.state } : { state, error: undefined },
        );
      });
      return true;
    } catch {
      this.#publish({ error: ERRORS.state });
      return false;
    }
  }

  async #read(request: Request, initial = false): Promise<boolean> {
    const currentError = () => (initial ? this.#current(request) : this.#latest(request));
    try {
      const state = await this.bridge.appUpdate(request.command);
      // The bridge returns this empty-version sentinel when the IPC reply was unavailable.
      if (unavailableReply(state)) {
        if (currentError()) this.#publish({ error: ERRORS[request.command.command] });
        return false;
      }
      if (this.#current(request)) this.#publish({ state, error: undefined });
      // Read, preference and control replies can retain an earlier check's error phase. A
      // download the network stopped waits as paused, with its reason.
      return (
        this.#mounted(request) &&
        (!phaseReportsFailure.has(request.command.command) ||
          (state.phase !== 'error' && state.error === null))
      );
    } catch {
      if (currentError()) this.#publish({ error: ERRORS[request.command.command] });
      return false;
    }
  }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    if (this.#listeners.size === 1) {
      this.#generation++;
      const initial = this.#request({ command: 'state' });
      // Capture the read's generation before subscribing: an immediate push wins too.
      if (this.#listen()) void this.#read(initial, true);
    }
    return () => {
      this.#listeners.delete(listener);
      if (this.#listeners.size !== 0) return;
      this.#generation++;
      const unsubscribe = this.#unsubscribe;
      this.#unsubscribe = undefined;
      unsubscribe?.();
    };
  };

  async send(command: AppUpdateCommand): Promise<boolean> {
    if (this.#listeners.size === 0 || this.#pause) return false;
    const interrupt =
      this.#active?.command.command === 'download' &&
      command.command === 'pause' &&
      this.#snapshot.state?.phase === 'downloading';
    if (this.#active && !interrupt) return false;
    const request = this.#request(command);
    if (command.command === 'state' && !this.#unsubscribe && !this.#listen()) return false;
    if (interrupt) this.#pause = request;
    else this.#active = request;
    this.#publish({ pending: command.command, error: undefined });
    try {
      return await this.#read(request);
    } finally {
      if (this.#pause === request) this.#pause = undefined;
      if (this.#active === request) this.#active = undefined;
      // A detached operation still owns the lock until its real Main transaction settles.
      this.#publish({ pending: this.#pause?.command.command ?? this.#active?.command.command });
    }
  }
}

const stores = new WeakMap<Bridge, Updates>();

export function useAppUpdates(): AppUpdateController {
  const bridge = window.aura.v1;
  let store = stores.get(bridge);
  if (!store) {
    store = new Updates(bridge);
    stores.set(bridge, store);
  }
  return useSyncExternalStore(store.subscribe, store.snapshot, store.snapshot);
}
