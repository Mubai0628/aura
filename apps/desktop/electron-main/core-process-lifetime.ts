import type { ChildProcess } from 'node:child_process';
import { waitForChildExit } from './child-exit.js';

interface ChildExit {
  code: number | null;
  signal: NodeJS.Signals | null;
}

/** Desktop owns at most one live child; a stop also retires starts waiting on that child. */
export class CoreProcessLifetime {
  #current: ChildProcess | undefined;
  #retiring: ChildProcess | undefined;
  #exit: ChildExit | undefined;
  #generation = 0;
  #stopEpoch = 0;
  #starting: Promise<void> | undefined;

  get child() {
    return this.#current;
  }
  get exit() {
    return this.#exit;
  }
  get generation() {
    return this.#generation;
  }

  start(spawn: () => Promise<void>): Promise<void> {
    if (this.#starting) return this.#starting;
    if (this.#current && this.#current.pid !== undefined && !this.#exit) return Promise.resolve();
    const epoch = this.#stopEpoch;
    const retiring = this.#retiring;
    const pending = Promise.resolve().then(async () => {
      if (retiring) await waitForChildExit(retiring);
      if (epoch !== this.#stopEpoch) return;
      this.#retiring = undefined;
      await spawn();
    });
    this.#starting = pending;
    const release = () => {
      if (this.#starting === pending) this.#starting = undefined;
    };
    void pending.then(release, release);
    return pending;
  }

  beginGeneration(): number {
    this.#exit = undefined;
    return ++this.#generation;
  }

  attach(child: ChildProcess): void {
    this.#current = child;
  }

  owns(generation: number, child: ChildProcess): boolean {
    return this.#generation === generation && this.#current === child;
  }

  acceptExit(generation: number, child: ChildProcess, exit: ChildExit): boolean {
    if (!this.owns(generation, child)) return false;
    this.#exit = exit;
    return true;
  }

  whenStopped(): Promise<void> {
    const child = this.#current ?? this.#retiring;
    return !child || child.pid === undefined ? Promise.resolve() : waitForChildExit(child);
  }

  stop(): void {
    this.#stopEpoch++;
    this.#starting = undefined;
    const child = this.#current;
    this.#current = undefined;
    if (!child) return;
    this.#retiring = child.pid !== undefined ? child : undefined;
    if (child.exitCode === null && !this.#exit && !child.killed) child.kill();
  }
}
