import type { IpcErrorEnvelope, RuntimeReadiness } from '@aura/contracts/generated';
import { useSyncExternalStore } from 'react';
import type { ResultEnvelope } from '../../shared/aura-api';
import { isRequestLocalTimeout } from '../../shared/ipc-request-failure';

interface ReadinessState {
  loading: boolean;
  value?: RuntimeReadiness;
  error?: string;
  /** The typed failure behind `error`, when the read produced one instead of rejecting. */
  errorEnvelope?: IpcErrorEnvelope | undefined;
}

/** A read that produced no envelope (the bridge itself rejected) is the connection-level case. */
const CONNECTION_ERROR = '运行状态暂时无法更新，请检查连接。';
/** One timed-out read leaves Core reachable: the budget expired, the connection did not fail. */
const REQUEST_TIMEOUT_ERROR = '运行状态查询超时，Core 连接仍然可用；请稍后刷新。';
/** Display cache only. Execution readiness is still checked by Core at dispatch. */
export class RuntimeReadinessStore {
  readonly #listeners = new Set<() => void>();
  #state: ReadinessState = { loading: true };
  #generation = 0;
  #connection: string | undefined;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #pending: Promise<void> | undefined;
  #checkedAt = 0;
  constructor(
    private readonly read: () => Promise<ResultEnvelope<RuntimeReadiness>>,
    private readonly interval = 30_000,
  ) {}
  getSnapshot = (): ReadinessState => this.#state;
  #notify() {
    for (const listener of this.#listeners) listener();
  }
  setConnection = (connection: string): void => {
    if (connection === this.#connection) return;
    this.#connection = connection;
    ++this.#generation;
    this.#checkedAt = 0;
    this.#state = { loading: true };
    clearTimeout(this.#timer);
    this.#notify();
    if (this.#listeners.size) void this.#poll();
  };
  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    if (this.#listeners.size === 1) this.#schedule();
    return () => {
      this.#listeners.delete(listener);
      if (!this.#listeners.size) clearTimeout(this.#timer);
    };
  };
  refresh = (): void => {
    this.#checkedAt = 0;
    clearTimeout(this.#timer);
    void this.#poll();
  };
  #schedule() {
    clearTimeout(this.#timer);
    if (!this.#listeners.size) return;
    const remaining = this.#checkedAt ? this.interval - (Date.now() - this.#checkedAt) : 0;
    if (remaining <= 0) void this.#poll();
    else this.#timer = setTimeout(() => void this.#poll(), remaining);
  }
  async #poll(): Promise<void> {
    if (this.#pending) {
      await this.#pending;
      if (!this.#checkedAt && this.#listeners.size) this.#schedule();
      return;
    }
    const generation = this.#generation;
    this.#pending = (async () => {
      let next: ReadinessState;
      try {
        const result = await this.read();
        next = result.ok
          ? { loading: false, value: result.value }
          : {
              ...this.#state,
              error: isRequestLocalTimeout(result.error) ? REQUEST_TIMEOUT_ERROR : CONNECTION_ERROR,
              errorEnvelope: result.error,
              loading: false,
            };
      } catch {
        next = {
          ...this.#state,
          error: CONNECTION_ERROR,
          errorEnvelope: undefined,
          loading: false,
        };
      }
      if (generation === this.#generation) {
        this.#state = next;
        this.#checkedAt = Date.now();
        this.#notify();
      }
    })();
    await this.#pending;
    this.#pending = undefined;
    this.#schedule();
  }
}
const store = new RuntimeReadinessStore(() => window.aura.v1.getRuntimeReadiness());
export const setReadinessConnection = store.setConnection;
export const refreshRuntimeReadiness = store.refresh;
export function useRuntimeReadiness(): ReadinessState {
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
