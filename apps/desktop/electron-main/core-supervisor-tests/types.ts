import type { Mock } from 'vitest';
import type { EventEmitter } from 'node:events';
import type {
  CommandOutcome,
  CommandInvocationId,
  CorrelationId,
  ProjectionChange,
  RuntimeHandshakeRequest,
  RuntimeHandshakeResponse,
} from '@aura/contracts/generated';
import type { CoreSupervisor } from '../core-supervisor.js';
import type { CoreTransport } from '../core-transport.js';
import type { Kind } from './fixture.js';

export type FixtureChild = EventEmitter & {
  killed: boolean;
  pid: number;
  exitCode: number | null;
  stdin: EventEmitter & { end: Mock<(...args: unknown[]) => void> };
  stdout: EventEmitter;
  stderr: EventEmitter;
  kill: Mock<() => boolean>;
};

export interface FixtureTransport {
  peerIdentity: { serverPid: number; serverCreationTime: string; serverWindowsSessionId: number };
  attachLifecycleObserver: Mock<(...args: unknown[]) => void>;
  cancel: Mock<(requestId: string) => Promise<{ request_id: string }>>;
  channelFacts: Mock<
    () => {
      authenticated: boolean;
      channel_id: string;
      closed: boolean;
      connection_epoch: number;
      observer_attached: boolean;
    }
  >;
  queryCommandOutcome: Mock<(commandInvocationId: string) => Promise<CommandOutcome>>;
  close: Mock<() => void>;
  finishHandshake: Mock<(...args: unknown[]) => void>;
  requestWithSecretInput: Mock<
    (method: string, params: unknown, operation: unknown, secret: Buffer) => Promise<unknown>
  >;
  beginHandshake: Mock<
    (
      request: RuntimeHandshakeRequest,
      budgetMilliseconds?: number,
    ) => Promise<RuntimeHandshakeResponse>
  >;
  request: Mock<
    (
      method: string,
      params: { projection_kind: Kind },
      priority?: number,
      timeoutMilliseconds?: number,
      onInvocation?: (id: CommandInvocationId) => void,
      onRequestId?: (id: CorrelationId) => void,
      onQueuedOpening?: (cancelIfUnsent: () => boolean) => void,
    ) => Promise<unknown>
  >;
}

export interface SupervisorFixture {
  supervisor: CoreSupervisor;
  child: FixtureChild;
  children: FixtureChild[];
  onEscalated: Mock<() => void>;
  transport: FixtureTransport;
  snapshot: (kind: Kind, at?: number) => ProjectionChange;
  connectAttempts: () => number;
  callbacks: () => Parameters<typeof CoreTransport.connect>[1];
  setRead: (handler: ((kind: Kind) => Promise<ProjectionChange>) | undefined) => void;
  push: (kind: Kind, at: number) => void;
}
