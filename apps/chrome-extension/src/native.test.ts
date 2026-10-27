import { describe, expect, it, vi } from 'vitest';
import { authenticationMac, bytesToBase64 } from './authentication.js';
import { NativeConnection, type ConnectionState, type NativePort } from './native.js';
import type { CoreMessage } from './protocol.js';

const ID = 'bheiijocpacefhgapeegbnekchcpedkj';

class FakePort implements NativePort {
  readonly sent: unknown[] = [];
  readonly #messages: ((message: unknown) => void)[] = [];
  readonly #closes: (() => void)[] = [];
  postMessage(message: unknown): void {
    this.sent.push(message);
  }
  disconnect(): void {}
  onMessage = {
    addListener: (listener: (message: unknown) => void) => void this.#messages.push(listener),
  };
  onDisconnect = { addListener: (listener: () => void) => void this.#closes.push(listener) };
  receive(message: CoreMessage): void {
    for (const listener of this.#messages) listener(message);
  }
  close(): void {
    for (const listener of this.#closes) listener();
  }
}

function harness(secret: Uint8Array<ArrayBuffer> | null = null) {
  const ports: FakePort[] = [];
  const states: ConnectionState[] = [];
  const forwarded: CoreMessage[] = [];
  const timers: { run: () => void; ms: number }[] = [];
  const store = { secret };
  let error: string | undefined;
  const connection = new NativeConnection({
    hostName: 'com.aura.chrome_control.ttest',
    extensionId: ID,
    version: '0.1.3',
    connect: () => {
      const port = new FakePort();
      ports.push(port);
      return port;
    },
    lastError: () => error,
    secrets: {
      load: async () => store.secret,
      save: async (value) => {
        store.secret = value;
      },
      forget: async () => {
        store.secret = null;
      },
    },
    onMessage: (message) => forwarded.push(message),
    onState: (state) => states.push(state),
    setTimer: (run, ms) => timers.push({ run, ms }),
    clearTimer: () => undefined,
  });
  return {
    connection,
    ports,
    states,
    forwarded,
    timers,
    store,
    fail(message: string) {
      error = message;
    },
  };
}

const settled = () => new Promise((done) => setTimeout(done, 0));

describe('the connection to Aura', () => {
  it('says hello, pairs with the code, keeps the secret and proves it', async () => {
    const h = harness();
    h.connection.start();
    const port = h.ports[0]!;
    expect(port.sent[0]).toEqual({ type: 'hello', extension_id: ID, extension_version: '0.1.3' });
    port.receive({ type: 'pairing_required' });
    await settled();
    expect(h.connection.state).toEqual({ kind: 'pairing_required', refused: null });
    h.connection.pair('12345678');
    expect(port.sent[1]).toEqual({ type: 'pair', code: '12345678' });
    const secret = new Uint8Array(32).fill(9);
    port.receive({ type: 'paired', secret: bytesToBase64(secret) });
    await settled();
    expect(Array.from(h.store.secret ?? [])).toEqual(Array.from(secret));
    const nonce = new Uint8Array([1, 2, 3]);
    port.receive({ type: 'challenge', nonce: bytesToBase64(nonce), protocol: 1 });
    const mac = await authenticationMac(secret, nonce, ID);
    await vi.waitFor(() => expect(port.sent[2]).toEqual({ type: 'authenticate', mac }));
    port.receive({ type: 'authenticated' });
    await settled();
    expect(h.connection.state).toEqual({ kind: 'connected' });
    expect(port.sent[3]).toEqual({ type: 'list_sessions' });
  });

  it('answers the heartbeat and passes on what it does not answer itself', async () => {
    const h = harness(new Uint8Array(32));
    h.connection.start();
    const port = h.ports[0]!;
    port.receive({ type: 'ping', id: 4 });
    port.receive({ type: 'cancel', id: 2 });
    await settled();
    expect(port.sent).toContainEqual({ type: 'pong', id: 4 });
    expect(h.forwarded).toEqual([{ type: 'cancel', id: 2 }]);
  });

  it('forgets the secret when Aura revokes it or no longer knows it', async () => {
    for (const message of [{ type: 'revoked' }, { type: 'pairing_required' }] as const) {
      const h = harness(new Uint8Array(32).fill(1));
      h.connection.start();
      h.ports[0]!.receive(message);
      await settled();
      expect(h.store.secret).toBeNull();
      expect(h.connection.state.kind).toBe('pairing_required');
    }
  });

  it('shows Aura not running or not set up, and tries again after a growing pause', async () => {
    const h = harness();
    h.connection.start();
    h.fail('Specified native messaging host not found.');
    h.ports[0]!.close();
    expect(h.connection.state).toEqual({ kind: 'not_registered' });
    h.timers[0]!.run();
    h.fail('Native host has exited.');
    h.ports[1]!.close();
    expect(h.connection.state).toEqual({ kind: 'not_running' });
    expect(h.timers.map((timer) => timer.ms)).toEqual([1_000, 2_000]);
    expect(h.connection.send({ type: 'stop' })).toBe(false);
  });
});
