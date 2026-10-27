/**
 * The connection to Aura through the relay Chrome starts: hello, then the pairing code or the
 * proof of an earlier pairing, then commands. Aura not running, or Chrome control switched off in
 * Aura, is a state the side panel shows; the connection is tried again after a growing pause.
 */
import { authenticationMac, base64ToBytes } from './authentication.js';
import type { AuthRefusal, CoreMessage, ExtensionMessage, PairRefusal } from './protocol.js';

export type ConnectionState =
  | { kind: 'connecting' }
  /** Chrome found no relay registered: Chrome control is off in Aura, or Aura is not installed. */
  | { kind: 'not_registered' }
  | { kind: 'not_running' }
  | { kind: 'pairing_required'; refused: PairRefusal | null }
  | { kind: 'refused'; reason: AuthRefusal }
  | { kind: 'connected' };

/** The parts of a `chrome.runtime.Port` this uses. */
export interface NativePort {
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: { addListener(listener: (message: unknown) => void): void };
  onDisconnect: { addListener(listener: () => void): void };
}

export interface ConnectionOptions {
  hostName: string;
  extensionId: string;
  version: string;
  /** `chrome.runtime.connectNative`, and the error Chrome gave when a port closed. */
  connect(hostName: string): NativePort;
  lastError(): string | undefined;
  secrets: SecretStore;
  /** Every Core message the connection itself does not answer. */
  onMessage(message: CoreMessage): void;
  onState(state: ConnectionState): void;
  setTimer(run: () => void, ms: number): unknown;
  clearTimer(timer: unknown): void;
}

/** The pairing secret, kept in `chrome.storage.local`. */
export interface SecretStore {
  load(): Promise<Uint8Array<ArrayBuffer> | null>;
  save(secret: Uint8Array<ArrayBuffer>): Promise<void>;
  forget(): Promise<void>;
}

const FIRST_RETRY_MS = 1_000;
const LONGEST_RETRY_MS = 60_000;

export class NativeConnection {
  readonly #options: ConnectionOptions;
  #port: NativePort | null = null;
  #state: ConnectionState = { kind: 'connecting' };
  #retryMs = FIRST_RETRY_MS;
  #retry: unknown = null;

  constructor(options: ConnectionOptions) {
    this.#options = options;
  }

  get state(): ConnectionState {
    return this.#state;
  }

  /** Connects now, unless a connection is open or about to be. */
  start(): void {
    if (this.#port) return;
    if (this.#retry !== null) {
      this.#options.clearTimer(this.#retry);
      this.#retry = null;
    }
    const port = this.#options.connect(this.#options.hostName);
    this.#port = port;
    this.#set({ kind: 'connecting' });
    port.onMessage.addListener((message) => {
      if (this.#port === port) void this.#received(message as CoreMessage);
    });
    port.onDisconnect.addListener(() => {
      if (this.#port === port) this.#closed(this.#options.lastError());
    });
    this.#post({
      type: 'hello',
      extension_id: this.#options.extensionId,
      extension_version: this.#options.version,
    });
  }

  /** Sends to Core; false when no connection is open. */
  send(message: ExtensionMessage): boolean {
    if (!this.#port || this.#state.kind !== 'connected') return false;
    this.#post(message);
    return true;
  }

  pair(code: string): void {
    if (this.#port && this.#state.kind === 'pairing_required') this.#post({ type: 'pair', code });
  }

  #post(message: ExtensionMessage): void {
    this.#port?.postMessage(message);
  }

  #set(state: ConnectionState): void {
    this.#state = state;
    this.#options.onState(state);
  }

  async #received(message: CoreMessage): Promise<void> {
    switch (message.type) {
      case 'pairing_required':
        // A secret Core no longer knows is worth nothing; the user pairs again.
        await this.#options.secrets.forget();
        this.#set({ kind: 'pairing_required', refused: null });
        return;
      case 'pair_refused':
        this.#set({ kind: 'pairing_required', refused: message.reason });
        return;
      case 'paired':
        await this.#options.secrets.save(base64ToBytes(message.secret));
        return;
      case 'challenge': {
        const secret = await this.#options.secrets.load();
        if (!secret) {
          this.#set({ kind: 'pairing_required', refused: null });
          return;
        }
        const mac = await authenticationMac(
          secret,
          base64ToBytes(message.nonce),
          this.#options.extensionId,
        );
        this.#post({ type: 'authenticate', mac });
        return;
      }
      case 'authenticated':
        this.#retryMs = FIRST_RETRY_MS;
        this.#set({ kind: 'connected' });
        this.#post({ type: 'list_sessions' });
        return;
      case 'auth_refused':
        if (message.reason !== 'protocol_unsupported') await this.#options.secrets.forget();
        this.#set({ kind: 'refused', reason: message.reason });
        return;
      case 'revoked':
        await this.#options.secrets.forget();
        this.#set({ kind: 'pairing_required', refused: null });
        return;
      case 'ping':
        this.#post({ type: 'pong', id: message.id });
        return;
      default:
        this.#options.onMessage(message);
    }
  }

  #closed(error: string | undefined): void {
    this.#port = null;
    const unregistered = error?.toLowerCase().includes('not found') ?? false;
    // A refusal or a revocation stays on screen until the next connection says otherwise.
    if (this.#state.kind !== 'refused' && this.#state.kind !== 'pairing_required') {
      this.#set({ kind: unregistered ? 'not_registered' : 'not_running' });
    }
    this.#retry = this.#options.setTimer(() => {
      this.#retry = null;
      this.start();
    }, this.#retryMs);
    this.#retryMs = Math.min(this.#retryMs * 2, LONGEST_RETRY_MS);
  }
}
