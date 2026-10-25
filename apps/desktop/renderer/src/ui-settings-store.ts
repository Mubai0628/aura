import type { UiSettings, UiSettingsSnapshot } from '@aura/contracts/generated';
import { useEffect, useSyncExternalStore } from 'react';
import { userMessage } from './agent-session/user-message';

/**
 * 设置 → 环境: the interface settings Core keeps — whether the session shows its debugging
 * information and which terminal a terminal tab opens. One store serves the window; what it shows
 * before Core answers, or when it cannot, is the defaults.
 */
export const DEFAULT_UI_SETTINGS: UiSettings = {
  show_debug_information: false,
  default_terminal: 'power_shell',
};

export interface UiSettingsState {
  loading: boolean;
  snapshot?: UiSettingsSnapshot | undefined;
  error?: string | undefined;
}

class UiSettingsStore {
  readonly #listeners = new Set<() => void>();
  #state: UiSettingsState = { loading: true };
  #readers = 0;
  /** Each read and save is numbered; an older answer never replaces a newer one. */
  #generation = 0;

  getSnapshot = (): UiSettingsState => this.#state;

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };

  start(): void {
    this.#readers += 1;
    if (this.#readers === 1 || this.#state.error !== undefined) void this.reload();
  }

  stop(): void {
    this.#readers = Math.max(0, this.#readers - 1);
  }

  reload = async (): Promise<void> => {
    const generation = ++this.#generation;
    try {
      const result = await window.aura.v1.getUiSettings();
      if (generation !== this.#generation) return;
      this.#set(
        result.ok
          ? { loading: false, snapshot: result.value }
          : {
              loading: false,
              snapshot: this.#state.snapshot,
              error: userMessage(result.error, '界面设置读取失败，请重试。'),
            },
      );
    } catch (reason) {
      if (generation !== this.#generation) return;
      this.#set({
        loading: false,
        snapshot: this.#state.snapshot,
        error: userMessage(reason, '界面设置读取失败，请重试。'),
      });
    }
  };

  /** Saves at the revision shown; answers the reason it was not saved, if it was not. */
  save = async (settings: UiSettings): Promise<string | undefined> => {
    const revision = this.#state.snapshot?.revision;
    if (revision == null) return '界面设置尚未读取，请稍后再改。';
    const generation = ++this.#generation;
    try {
      const result = await window.aura.v1.updateUiSettings({
        expected_revision: revision,
        settings,
      });
      if (result.ok) {
        if (generation === this.#generation) this.#set({ loading: false, snapshot: result.value });
        return undefined;
      }
      void this.reload();
      return userMessage(result.error, '设置没有保存，请重试。');
    } catch (reason) {
      void this.reload();
      return userMessage(reason, '设置没有保存，请重试。');
    }
  };

  resetForTests(): void {
    this.#readers = 0;
    this.#generation += 1;
    this.#state = { loading: true };
  }

  #set(state: UiSettingsState): void {
    this.#state = state;
    for (const listener of this.#listeners) listener();
  }
}

const store = new UiSettingsStore();

export function useUiSettings() {
  useEffect(() => {
    store.start();
    return () => store.stop();
  }, []);
  const state = useSyncExternalStore(store.subscribe, store.getSnapshot);
  return {
    ...state,
    settings: state.snapshot?.settings ?? DEFAULT_UI_SETTINGS,
    save: store.save,
    reload: store.reload,
  };
}

export function resetUiSettingsForTests(): void {
  store.resetForTests();
}
