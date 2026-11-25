import type { CoreTransport } from './core-transport.js';

/** One transport becomes active only after the supervisor has authenticated it. */
export class CoreConnectionLifetime {
  #generation = 0;
  #activeGeneration = 0;
  #epoch = 0;
  #transport: CoreTransport | undefined;

  get generation() {
    return this.#generation;
  }
  get activeGeneration() {
    return this.#activeGeneration;
  }
  get epoch() {
    return this.#epoch;
  }
  get transport() {
    return this.#transport;
  }

  nextAttempt(): number {
    return ++this.#generation;
  }

  replace(transport: CoreTransport, generation: number, epoch: number): void {
    const previous = this.#transport;
    this.#transport = transport;
    this.#activeGeneration = generation;
    this.#epoch = epoch;
    if (previous && previous !== transport) previous.close();
  }

  observeClose(generation: number): boolean {
    if (generation !== this.#activeGeneration) return false;
    this.#activeGeneration = 0;
    return true;
  }

  close(newProcess = false): void {
    this.#generation++;
    this.#activeGeneration = 0;
    if (newProcess) this.#epoch = 0;
    const retired = this.#transport;
    this.#transport = undefined;
    retired?.close();
  }
}
