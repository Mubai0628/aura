import { createRequire } from 'node:module';
import type { CoreProcessActivity } from './core-supervisor/responsiveness.js';

const EXPECTED_BRIDGE_ABI_VERSION = 1;
const EXPECTED_NAPI_VERSION = 10;
const EXPECTED_PROTOCOL = { major: 1, minor: 7 } as const;

export interface MainPipeConnectionInfo {
  serverCreationTime: string;
  serverPid: number;
  serverWindowsSessionId: number;
}

export interface MainPipeConnection {
  cancel(): boolean;
  close(): void;
  info(): MainPipeConnectionInfo;
  read(maximumBytes: number): Promise<Buffer>;
  write(bytes: Buffer): Promise<void>;
}

interface MainPipeBridgeInfo {
  bridgeAbiVersion: number;
  napiVersion: number;
  protocolMajor: number;
  protocolMinor: number;
  target: string;
}

interface MainPipeBridgeAddon {
  connect(options: {
    expectedServerCreationTime?: string;
    expectedServerPid: number;
    expectedServerWindowsSessionId?: number;
    namespaceObjectHash: string;
    pipeNonce: string;
    runtimeInstanceId: string;
  }): Promise<MainPipeConnection>;
  getBridgeInfo(): MainPipeBridgeInfo;
  /** Added after ABI 1 was frozen: a bridge built before it has no such export. */
  processActivity?: (pid: number, creationTime: string) => unknown;
}

const require = createRequire(import.meta.url);
const loaded = new Map<string, MainPipeBridgeAddon>();

export const connectMainPipe = async (options: {
  addonPath: string;
  expectedServerCreationTime?: string;
  expectedServerPid: number;
  expectedServerWindowsSessionId?: number;
  namespaceObjectHash: string;
  pipeNonce: string;
  runtimeInstanceId: string;
}): Promise<MainPipeConnection> => {
  const addon = loadMainPipeBridge(options.addonPath);
  const endpoint: Parameters<MainPipeBridgeAddon['connect']>[0] = {
    expectedServerPid: options.expectedServerPid,
    namespaceObjectHash: options.namespaceObjectHash,
    pipeNonce: options.pipeNonce,
    runtimeInstanceId: options.runtimeInstanceId,
  };
  if (options.expectedServerCreationTime !== undefined) {
    endpoint.expectedServerCreationTime = options.expectedServerCreationTime;
  }
  if (options.expectedServerWindowsSessionId !== undefined) {
    endpoint.expectedServerWindowsSessionId = options.expectedServerWindowsSessionId;
  }
  const connection = await addon.connect(endpoint);
  const info = connection.info();
  if (
    info.serverPid !== options.expectedServerPid ||
    !/^\d+$/u.test(info.serverCreationTime) ||
    !Number.isSafeInteger(info.serverWindowsSessionId) ||
    (options.expectedServerCreationTime !== undefined &&
      info.serverCreationTime !== options.expectedServerCreationTime) ||
    (options.expectedServerWindowsSessionId !== undefined &&
      info.serverWindowsSessionId !== options.expectedServerWindowsSessionId)
  ) {
    connection.close();
    throw new Error('Aura Main Pipe Bridge returned an invalid Core process identity');
  }
  return connection;
};

/**
 * The Core process's CPU time and completed reads and writes, read by the process identity the
 * pipe was verified against. `undefined` when the bridge cannot read them or the process is gone.
 */
export const readCoreProcessActivity = (
  addonPath: string,
  pid: number,
  creationTime: string,
): CoreProcessActivity | undefined => {
  try {
    const read = loadMainPipeBridge(addonPath).processActivity?.(pid, creationTime);
    if (typeof read !== 'object' || read === null) return undefined;
    const { cpuTime, ioOperations } = read as Record<string, unknown>;
    if (typeof cpuTime !== 'string' || !/^\d+$/u.test(cpuTime)) return undefined;
    if (typeof ioOperations !== 'string' || !/^\d+$/u.test(ioOperations)) return undefined;
    return { cpuTime: BigInt(cpuTime), ioOperations: BigInt(ioOperations) };
  } catch {
    return undefined;
  }
};

const loadMainPipeBridge = (addonPath: string): MainPipeBridgeAddon => {
  if (process.platform !== 'win32' || process.arch !== 'x64') {
    throw new Error('Aura Main Pipe Bridge only supports win32-x64');
  }
  if (Number(process.versions.napi) !== EXPECTED_NAPI_VERSION) {
    throw new Error(`Aura Main Pipe Bridge requires Node-API ${EXPECTED_NAPI_VERSION}`);
  }
  const existing = loaded.get(addonPath);
  if (existing !== undefined) return existing;
  const candidate = require(addonPath) as unknown;
  if (!isBridgeAddon(candidate)) {
    throw new Error('Aura Main Pipe Bridge exports are invalid');
  }
  const info = candidate.getBridgeInfo();
  if (
    info.bridgeAbiVersion !== EXPECTED_BRIDGE_ABI_VERSION ||
    info.napiVersion !== EXPECTED_NAPI_VERSION ||
    info.target !== 'win32-x64' ||
    info.protocolMajor !== EXPECTED_PROTOCOL.major ||
    info.protocolMinor !== EXPECTED_PROTOCOL.minor
  ) {
    throw new Error('Aura Main Pipe Bridge ABI or protocol version is incompatible');
  }
  loaded.set(addonPath, candidate);
  return candidate;
};

const isBridgeAddon = (value: unknown): value is MainPipeBridgeAddon =>
  typeof value === 'object' &&
  value !== null &&
  typeof (value as Record<string, unknown>).connect === 'function' &&
  typeof (value as Record<string, unknown>).getBridgeInfo === 'function';
