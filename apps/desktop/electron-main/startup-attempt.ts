/**
 * One Core startup attempt owns its generation, cancel signal, and any transport
 * that exists before activation. Stop, an old generation, or an early process
 * exit abandons the attempt: the held connection is closed and a late result
 * cannot activate it.
 */

export class StartupAttempt {
  readonly generation: number;
  readonly startedAt = performance.now();
  readonly abort = new AbortController();
  #held: { close(): void } | undefined;
  #released = false;
  #abandoned = false;

  constructor(generation: number) {
    this.generation = generation;
  }

  elapsedMilliseconds(): number {
    return Math.max(0, Math.round(performance.now() - this.startedAt));
  }

  get abandoned(): boolean {
    return this.#abandoned;
  }

  /** Keeps a pre-activation transport on this attempt so stop can close it. */
  holdTransport(transport: { close(): void }): void {
    if (this.#abandoned || this.#released) {
      transport.close();
      return;
    }
    this.#held = transport;
  }

  /** Hands the held transport to the supervisor. After this, stop closes `#transport`. */
  releaseForActivation(): boolean {
    if (this.#abandoned) return false;
    this.#held = undefined;
    this.#released = true;
    return true;
  }

  abandon(): void {
    if (this.#abandoned) return;
    this.#abandoned = true;
    this.abort.abort();
    if (!this.#released) this.#held?.close();
    this.#held = undefined;
  }
}

export const startupStillOwns = (
  attempt: StartupAttempt,
  generation: number,
  stopping: boolean,
  childExited: boolean,
): boolean => attempt.generation === generation && !attempt.abandoned && !stopping && !childExited;

/** A bounded attempt outcome: an abandoned attempt is closed by its own drain. */
export type BoundedAttempt<Value> =
  { kind: 'value'; value: Value } | { kind: 'error'; error: unknown } | { kind: 'ended' };

/**
 * Bounds one non-cancellable attempt by the remaining startup budget. The native
 * pipe connect runs on its own thread with a fixed internal wait, so the phase
 * abandons an attempt that overruns and hands any late success to `onLateValue`
 * instead of ever letting it become the active connection.
 */
export const settleWithin = <Value>(
  work: Promise<Value>,
  milliseconds: number,
  signal: AbortSignal | undefined,
  onLateValue: (value: Value) => void,
): Promise<BoundedAttempt<Value>> =>
  new Promise((resolve) => {
    let timer: NodeJS.Timeout | undefined = undefined;
    let settled = false;
    const onAbort = () => finish({ kind: 'ended' });
    const cleanup = () => {
      if (timer !== undefined) clearTimeout(timer);
      signal?.removeEventListener('abort', onAbort);
    };
    function finish(outcome: BoundedAttempt<Value>): void {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(outcome);
    }
    if (signal?.aborted === true) {
      finish({ kind: 'ended' });
      return;
    }
    signal?.addEventListener('abort', onAbort, { once: true });
    timer = setTimeout(() => finish({ kind: 'ended' }), Math.max(0, milliseconds));
    work.then(
      (value) => {
        if (settled) onLateValue(value);
        else finish({ kind: 'value', value });
      },
      (error: unknown) => finish({ kind: 'error', error }),
    );
  });

/** Bounds the authenticated handshake on an already reachable process. */
export const withDeadline = async <Value>(
  work: Promise<Value>,
  milliseconds: number,
): Promise<Value> => {
  let timer: NodeJS.Timeout | undefined;
  try {
    return await Promise.race([
      work,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () =>
            reject(
              Object.assign(
                new Error(`Aura Core handshake did not complete within ${milliseconds}ms`),
                { code: 'ETIMEDOUT' },
              ),
            ),
          milliseconds,
        );
      }),
    ]);
  } finally {
    if (timer !== undefined) clearTimeout(timer);
  }
};
