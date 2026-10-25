import type {
  RuntimeHandshake,
  RuntimeHandshakeRequest,
  RuntimeHandshakeResponse,
  RuntimeHandshakeTranscript,
} from '@aura/contracts/generated';
import { PROTOCOL_VERSION, type CoreSupervisorOptions } from './config.js';
import { CompatibilityFailure, PairingInvalidated } from './errors.js';
import {
  hashRuntimeHandshakeTranscript,
  isLowerHex,
  isRecord,
  sameRuntimeHandshake,
} from './handshake.js';

/** The handshake Main presents to Core: its release, its runtime instance and its features. */
export function mainRuntimeHandshake(
  options: CoreSupervisorOptions,
  mainRuntimeInstanceId: string,
): RuntimeHandshake {
  return {
    build_commit: options.release.manifest.build_commit,
    component_version: options.release.desktopComponent.version,
    parent_runtime_instance_id: options.parentRuntimeInstanceId ?? null,
    process_kind: 'electron_main',
    protocol_version: PROTOCOL_VERSION,
    release_descriptor: options.release.releaseDescriptor,
    runtime_instance_id: mainRuntimeInstanceId,
    supported_features: [
      'aura.ipc.v1',
      'aura.ipc.authenticated-channel.v1',
      'aura.ipc.command-outcome.v1',
      'aura.ipc.secret-input.v1',
      'aura.ipc.tool-secret-input.v1',
      'aura.main-pipe-bridge.v1',
      'aura.preload.v1',
      'aura.message-port.v1',
      'aura.projection.workspace-session.v1',
      'aura.projection.workspace-files.v1',
      'aura.security-control-plane.v1',
      'aura.model-control-plane.v1',
      'aura.tool-control-plane.v1',
    ],
  };
}

/** What Core's handshake is checked against. */
export interface HandshakeExpectation {
  coreRuntimeInstanceId: string;
  mainRuntimeInstanceId: string;
  release: CoreSupervisorOptions['release'];
  /** The Core handshake frozen at startup; a reconnect must present the same one. */
  frozenCore: RuntimeHandshake | undefined;
}

/** The optional parts of the protocol this Core offers. */
export interface CoreFeatureSupport {
  workspaceSession: boolean;
  workspaceFiles: boolean;
  security: boolean;
  model: boolean;
  tool: boolean;
  toolCredentials: boolean;
}

/**
 * Checks Core's handshake response against the request and what Main expects: the response is
 * well formed and bound to this channel, epoch and nonce; the protocol versions agree; both come
 * from the same verified Release Bundle; a reconnect presents the frozen handshake; the transcript
 * hash matches; the features the negotiated minor version requires are present. Throws
 * `CompatibilityFailure` or `PairingInvalidated`; otherwise returns the transcript hash and the
 * optional features.
 */
export function validateCoreHandshake(
  response: RuntimeHandshakeResponse,
  request: RuntimeHandshakeRequest,
  expected: HandshakeExpectation,
): { transcriptHash: RuntimeHandshakeResponse['transcript_hash']; features: CoreFeatureSupport } {
  if (
    !isRecord(response) ||
    !isRecord(response.handshake) ||
    !isRecord(response.handshake.protocol_version) ||
    !isRecord(response.handshake.release_descriptor) ||
    typeof response.handshake.release_descriptor.bundle_namespace !== 'string' ||
    !isRecord(response.handshake.release_descriptor.identity) ||
    typeof response.handshake.release_descriptor.identity.release_bundle_id !== 'string' ||
    !isRecord(response.handshake.release_descriptor.identity.component_manifest_hash) ||
    typeof response.handshake.release_descriptor.identity.component_manifest_hash.algorithm !==
      'string' ||
    typeof response.handshake.release_descriptor.identity.component_manifest_hash.hex !==
      'string' ||
    !isRecord(response.negotiated_protocol_version) ||
    !isRecord(response.transcript_hash) ||
    typeof response.transcript_hash.algorithm !== 'string' ||
    typeof response.transcript_hash.hex !== 'string' ||
    typeof response.handshake_kind !== 'string' ||
    typeof response.channel_id !== 'string' ||
    typeof response.connection_epoch !== 'number' ||
    typeof response.client_nonce !== 'string' ||
    typeof response.server_nonce !== 'string' ||
    !Array.isArray(response.handshake.supported_features) ||
    !response.handshake.supported_features.every(
      (feature): feature is string => typeof feature === 'string',
    )
  ) {
    throw new CompatibilityFailure('Core 返回了无效的握手响应。');
  }
  const handshake = response.handshake;
  if (
    response.handshake_kind !== request.handshake_kind ||
    response.channel_id !== request.channel_id ||
    response.connection_epoch !== request.connection_epoch ||
    response.client_nonce !== request.client_nonce ||
    !isLowerHex(response.server_nonce, 64) ||
    response.transcript_hash.algorithm !== 'sha256' ||
    !isLowerHex(response.transcript_hash.hex, 64)
  ) {
    throw new CompatibilityFailure('Core 握手响应没有绑定当前 Channel、Epoch 和 Nonce。');
  }
  if (
    handshake.process_kind !== 'core' ||
    handshake.protocol_version.major !== PROTOCOL_VERSION.major ||
    response.negotiated_protocol_version.major !== PROTOCOL_VERSION.major
  ) {
    throw new CompatibilityFailure('Aura.exe 与 aura-core.exe 的 IPC 主版本不兼容。');
  }
  const expectedMinor = Math.min(PROTOCOL_VERSION.minor, Number(handshake.protocol_version.minor));
  if (
    !Number.isSafeInteger(handshake.protocol_version.minor) ||
    !Number.isSafeInteger(response.negotiated_protocol_version.minor) ||
    response.negotiated_protocol_version.minor !== expectedMinor
  ) {
    throw new CompatibilityFailure('Aura.exe 与 aura-core.exe 的 IPC 次版本协商无效。');
  }
  if (
    handshake.runtime_instance_id !== expected.coreRuntimeInstanceId ||
    handshake.parent_runtime_instance_id !== expected.mainRuntimeInstanceId ||
    handshake.release_descriptor.bundle_namespace !==
      expected.release.releaseDescriptor.bundle_namespace ||
    handshake.release_descriptor.identity.release_bundle_id !==
      expected.release.releaseDescriptor.identity.release_bundle_id ||
    handshake.release_descriptor.identity.component_manifest_hash.algorithm !==
      expected.release.componentManifestHash.algorithm ||
    handshake.release_descriptor.identity.component_manifest_hash.hex !==
      expected.release.componentManifestHash.hex
  ) {
    throw new CompatibilityFailure('Aura.exe 与 aura-core.exe 不属于同一已验证 Release Bundle。');
  }
  if (
    !expected.release.allowDevMismatch &&
    handshake.build_commit !== expected.release.manifest.build_commit
  ) {
    throw new CompatibilityFailure('生产组件 build_commit 与 Release Bundle 不一致。');
  }
  if (
    request.handshake_kind === 'reconnect' &&
    (expected.frozenCore === undefined || !sameRuntimeHandshake(expected.frozenCore, handshake))
  ) {
    throw new PairingInvalidated(
      'Core RuntimeInstanceId、ReleaseDescriptor 或固定握手状态发生变化',
    );
  }
  const transcript: RuntimeHandshakeTranscript = {
    channel_id: request.channel_id,
    client_handshake: request.handshake,
    client_nonce: request.client_nonce,
    connection_epoch: request.connection_epoch,
    handshake_kind: request.handshake_kind,
    negotiated_protocol_version: response.negotiated_protocol_version,
    server_handshake: handshake,
    server_nonce: response.server_nonce,
  };
  const expectedTranscriptHash: RuntimeHandshakeResponse['transcript_hash'] = {
    algorithm: 'sha256',
    hex: hashRuntimeHandshakeTranscript(transcript),
  };
  if (
    response.transcript_hash.algorithm !== expectedTranscriptHash.algorithm ||
    response.transcript_hash.hex !== expectedTranscriptHash.hex
  ) {
    throw new CompatibilityFailure('Core 握手 transcript hash 与本地冻结状态不一致。');
  }
  if (
    response.negotiated_protocol_version.minor >= 6 &&
    (!handshake.supported_features.includes('aura.ipc.authenticated-channel.v1') ||
      !handshake.supported_features.includes('aura.ipc.command-outcome.v1'))
  ) {
    throw new CompatibilityFailure('Core 未提供协议 1.6 所需的认证 Channel 或命令结果能力。');
  }
  if (
    response.negotiated_protocol_version.minor === 7 &&
    !handshake.supported_features.includes('aura.ipc.secret-input.v1')
  ) {
    throw new CompatibilityFailure('Core 未提供协议 1.7 所需的 SecretInput V1 能力。');
  }
  const tool =
    response.negotiated_protocol_version.minor >= 7 &&
    handshake.supported_features.includes('aura.tool-control-plane.v1');
  return {
    transcriptHash: expectedTranscriptHash,
    features: {
      workspaceSession:
        response.negotiated_protocol_version.minor >= 1 &&
        handshake.supported_features.includes('aura.projection.workspace-session.v1'),
      workspaceFiles:
        response.negotiated_protocol_version.minor >= 2 &&
        handshake.supported_features.includes('aura.projection.workspace-files.v1'),
      security:
        response.negotiated_protocol_version.minor >= 3 &&
        handshake.supported_features.includes('aura.security-control-plane.v1'),
      model:
        response.negotiated_protocol_version.minor >= 7 &&
        handshake.supported_features.includes('aura.model-control-plane.v1'),
      tool,
      toolCredentials:
        tool && handshake.supported_features.includes('aura.ipc.tool-secret-input.v1'),
    },
  };
}
