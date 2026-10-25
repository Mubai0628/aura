import {
  isRecoveryParams,
  parseRecoveryResult,
  type ConfigRecoveryMainCommand,
  type ConfigRecoveryMainParams,
  type ConfigRecoveryMainResult,
} from '../shared/recovery-validation.js';
import { mainRoute } from '../shared/generated/routes.js';
import { projectionCompatibilityBridge } from './projection-compatibility.js';
import { mainErrorsBridge } from './main-errors.js';
import { projectionListeners } from './projection-listeners.js';
import { isIpcErrorEnvelope, preloadError } from './errors.js';
import { toolCredentialBridge } from './tool-credentials.js';
import { agentBridge } from './agent.js';
import { appUpdateBridge } from './app-updates.js';
import { gitBridge } from './git.js';
import { packageBridge } from './package.js';
import { memoryBridge } from './memory.js';
import { sessionConsentsBridge } from './consents/bridge.js';
import { validateAgentResponse } from '../shared/agent-validation.js';
import { parseRuntimeReadiness } from '../shared/readiness-validation.js';
import type {
  IpcErrorEnvelope,
  PrepareEditorLifecycleTransition,
  ProjectionChange,
  ToolActivityPageRequestV1,
  ToolActivityPageV1,
  ToolCredentialCandidatesProjectionV1,
} from '@aura/contracts/generated';
import { contextBridge, ipcRenderer, type IpcRendererEvent } from 'electron';

import type {
  AuraApiV1,
  AuraBinaryStream,
  BinaryStreamChunk,
  BinaryStreamOpenRequest,
  CoreHealthProjectionChange,
  ModelProjectionChange,
  ModelProjectionRouteRequest,
  ResultEnvelope,
  SecurityControlProjectionChange,
  ToolProjectionChange,
  ToolProjectionRouteRequest,
  WorkspaceFilesProjectionChange,
  WorkspaceSessionProjectionChange,
} from '../shared/aura-api.js';
import {
  isPrepareEditorLifecycleTransition,
  isP03MainParams,
  type P03MainCommand,
  type P03MainParams,
  type P03MainResult,
} from '../shared/p03-validation.js';
import {
  isP04MainParams,
  parseSecurityProjection,
  type P04MainCommand,
  type P04MainParams,
  type P04MainResult,
} from '../shared/p04-validation.js';
import {
  isP05MainParams,
  isValidApiKeyBytes,
  type P05MainCommand,
  type P05MainParams,
  type P05MainResult,
  type P05NonSecretMainCommand,
  type P05SecretMainCommand,
} from '../shared/p05-validation.js';
import {
  isP07MainParams,
  isP07ReadParams,
  isToolMutationResult,
  type P07MainCommand,
  type P07MainParams,
  type P07MainResult,
  type P07ReadParams,
} from '../shared/p07-validation.js';
import {
  isModelProjectionKind,
  isModelProjectionRouteRequest,
  isToolProjectionKind,
  isToolProjectionRouteRequest,
  parseProjection,
  toolProjectionRouteKey,
} from '../shared/projection-validation.js';
import {
  isToolActivityPage,
  isToolActivityPageRequest,
  isToolCredentialCandidatesProjection,
} from '../shared/tool-projection-validation.js';

const INVOKE_CHANNEL = 'aura:v1:invoke';
const WORKSPACE_LIFECYCLE_PREPARE_EVENT_CHANNEL = 'aura:v1:workspace-lifecycle-prepare';
const SECURITY_CONTROL_EVENT_CHANNEL = 'aura:v1:security-control';
const MODEL_PROJECTION_EVENT_CHANNEL = 'aura:v1:model-projection';
const TOOL_PROJECTION_EVENT_CHANNEL = 'aura:v1:tool-projection';
const MODEL_SECRET_INVOKE_CHANNEL = 'aura:v1:model-secret';
const STREAM_OPEN_CHANNEL = 'aura:v1:stream-open';
const STREAM_PORT_CHANNEL = 'aura:v1:stream-port';
let streamRequestSequence = 0;

interface PendingStream {
  reject: (error: Error) => void;
  resolve: (result: ResultEnvelope<AuraBinaryStream>) => void;
  timeout: ReturnType<typeof setTimeout>;
}

const pendingStreams = new Map<string, PendingStream>();

ipcRenderer.on(STREAM_PORT_CHANNEL, (event: IpcRendererEvent, value: unknown) => {
  if (!isRecord(value) || typeof value.requestId !== 'string') return;
  const pending = pendingStreams.get(value.requestId);
  if (pending === undefined) return;
  pendingStreams.delete(value.requestId);
  clearTimeout(pending.timeout);
  const port = event.ports[0];
  if (port === undefined) {
    if (isIpcErrorEnvelope(value.error)) pending.resolve({ error: value.error, ok: false });
    else pending.reject(new Error('Main did not transfer a MessagePort for the stream'));
    return;
  }
  pending.resolve({ ok: true, value: wrapPort(port) });
});

const api: AuraApiV1 = Object.freeze({
  memory: memoryBridge(
    (command, params) => ipcRenderer.invoke(INVOKE_CHANNEL, { command, params }),
    preloadError,
    isIpcErrorEnvelope,
  ),
  mainErrors: mainErrorsBridge(),
  git: gitBridge(
    (command, params) => ipcRenderer.invoke(INVOKE_CHANNEL, { command, params }),
    preloadError,
    isIpcErrorEnvelope,
  ),
  packages: packageBridge(
    (command, params) => ipcRenderer.invoke(INVOKE_CHANNEL, { command, params }),
    preloadError,
    isIpcErrorEnvelope,
  ),
  ...projectionListeners(),
  projectionCompatibility: projectionCompatibilityBridge(isIpcErrorEnvelope, preloadError),
  ...sessionConsentsBridge((command, request) => invokeP04(command, request)),
  ...toolCredentialBridge(
    invokeRegistered,
    (value): value is IpcErrorEnvelope => isIpcErrorEnvelope(value),
    () =>
      preloadError(
        'invalid_tool_credential_boundary',
        'Invalid tool credential request or response.',
      ),
  ),
  getConfigRecoveryNotice: () => invokeRecovery('config.recovery.notice', {}),
  planConfigRecovery: (request: ConfigRecoveryMainParams['config.recovery.plan']) =>
    invokeRecovery('config.recovery.plan', request),
  applyConfigRecovery: (request: ConfigRecoveryMainParams['config.recovery.apply']) =>
    invokeRecovery('config.recovery.apply', request),

  ...appUpdateBridge(),
  ...agentBridge(
    invokeRegistered,
    (code, message) => preloadError(code, message),
    (value): value is IpcErrorEnvelope => isIpcErrorEnvelope(value),
  ),
  createSession: (request: P03MainParams['session.create']) => invokeP03('session.create', request),
  renameSession: (request: P03MainParams['session.rename']) => invokeP03('session.rename', request),
  archiveSession: (request: P03MainParams['session.archive']) =>
    invokeP03('session.archive', request),
  restoreSession: (request: P03MainParams['session.restore']) =>
    invokeP03('session.restore', request),
  deleteSession: (request: P03MainParams['session.delete']) => invokeP03('session.delete', request),
  attachExternalReadOnly: (request: P04MainParams['security.external_grant.attach_read_only']) =>
    invokeP04('security.external_grant.attach_read_only', request),
  acknowledgeWorkspaceLifecycle: (request: P03MainParams['workspace.lifecycle.ack']) =>
    invokeP03('workspace.lifecycle.ack', request),
  applyWorkspaceChangeset: (request: P03MainParams['fs.apply_changeset']) =>
    invokeP03('fs.apply_changeset', request),
  applyWorkspacePatch: (request: P03MainParams['fs.apply_patch']) =>
    invokeP03('fs.apply_patch', request),
  archiveWorkspace: (request: P03MainParams['workspace.archive']) =>
    invokeP03('workspace.archive', request),
  deleteWorkspace: (request: P03MainParams['workspace.delete']) =>
    invokeP03('workspace.delete', request),
  bindToolInstanceCredential: (request: P07MainParams['tools.instance.bind_credential']) =>
    invokeP07('tools.instance.bind_credential', request),
  clearSessionToolPin: (request: P07MainParams['tools.pin.session.clear']) =>
    invokeP07('tools.pin.session.clear', request),
  clearToolInstanceCredential: (request: P07MainParams['tools.instance.clear_credential']) =>
    invokeP07('tools.instance.clear_credential', request),
  clearWorkspaceToolPin: (request: P07MainParams['tools.pin.workspace.clear']) =>
    invokeP07('tools.pin.workspace.clear', request),
  setSessionToolPin: (request: P07MainParams['tools.pin.session.set']) =>
    invokeP07('tools.pin.session.set', request),
  setToolEnabled: (request: P07MainParams['tools.tool.set_enabled']) =>
    invokeP07('tools.tool.set_enabled', request),
  setWorkspaceToolPin: (request: P07MainParams['tools.pin.workspace.set']) =>
    invokeP07('tools.pin.workspace.set', request),
  updateToolInstanceConfiguration: (
    request: P07MainParams['tools.instance.update_configuration'],
  ) => invokeP07('tools.instance.update_configuration', request),
  clearModelCredential: (request: P05MainParams['models.endpoint.clear_credential']) =>
    invokeP05('models.endpoint.clear_credential', request),
  clearModelContextWindowOverride: (
    request: P05MainParams['models.context_window.clear_user_override'],
  ) => invokeP05('models.context_window.clear_user_override', request),
  configureModelCredential: (
    request: P05MainParams['models.endpoint.configure_credential'],
    secret: Uint8Array,
  ) => invokeP05Secret('models.endpoint.configure_credential', request, secret),
  correctModelContextWindow: (request: P05MainParams['models.context_window.correct']) =>
    invokeP05('models.context_window.correct', request),
  createModelEndpoint: (request: P05MainParams['models.endpoint.create']) =>
    invokeP05('models.endpoint.create', request),
  createModelEndpointWithCredential: (
    request: P05MainParams['models.endpoint.create_with_credential'],
    secret: Uint8Array,
  ) => invokeP05Secret('models.endpoint.create_with_credential', request, secret),
  createModelProfile: (request: P05MainParams['models.profile.create_manual']) =>
    invokeP05('models.profile.create_manual', request),
  createModelProvider: (request: P05MainParams['models.provider.create']) =>
    invokeP05('models.provider.create', request),
  createWorkspace: (request: P03MainParams['workspace.create']) =>
    invokeP03('workspace.create', request),
  selectWorkspaceDirectory: () => invokeP03('workspace.select_directory', {}),
  deriveWorkspaceExternalGrants: (
    request: P04MainParams['security.external_grant.derive_workspace'],
  ) => invokeP04('security.external_grant.derive_workspace', request),
  discardQuarantine: (request: P04MainParams['security.quarantine.discard']) =>
    invokeP04('security.quarantine.discard', request),
  copyWorkspacePath: (request: P03MainParams['fs.copy']) => invokeP03('fs.copy', request),
  createWorkspaceDirectory: (request: P03MainParams['fs.create_directory']) =>
    invokeP03('fs.create_directory', request),
  deleteWorkspacePath: (request: P03MainParams['fs.delete']) => invokeP03('fs.delete', request),
  deleteModelEndpoint: (request: P05MainParams['models.endpoint.delete']) =>
    invokeP05('models.endpoint.delete', request),
  deleteModelProfile: (request: P05MainParams['models.profile.delete']) =>
    invokeP05('models.profile.delete', request),
  deleteModelProvider: (request: P05MainParams['models.provider.delete']) =>
    invokeP05('models.provider.delete', request),
  startModelDiscovery: (request: P05MainParams['models.discovery.start']) =>
    invokeP05('models.discovery.start', request),
  cancelModelDiscovery: (request: P05MainParams['models.discovery.cancel']) =>
    invokeP05('models.discovery.cancel', request),
  cancelModelAvailabilityTest: (request: P05MainParams['models.availability_test.cancel']) =>
    invokeP05('models.availability_test.cancel', request),
  getModelCatalogProvider: (request: P05MainParams['models.catalog.provider.get']) =>
    invokeP05('models.catalog.provider.get', request),
  createModelProfileFromCatalog: (request: P05MainParams['models.profile.create_from_catalog']) =>
    invokeP05('models.profile.create_from_catalog', request),
  applyModelProfileCatalogUpdate: (request: P05MainParams['models.profile.apply_catalog_update']) =>
    invokeP05('models.profile.apply_catalog_update', request),
  editWorkspaceBuffer: (request: P03MainParams['workspace.buffer.edit']) =>
    invokeP03('workspace.buffer.edit', request),
  exportArtifact: (request: P04MainParams['artifact.export']) =>
    invokeP04('artifact.export', request),
  findWorkspacePaths: (request: P03MainParams['fs.find_paths']) =>
    invokeP03('fs.find_paths', request),
  globWorkspacePaths: (request: P03MainParams['fs.glob']) => invokeP03('fs.glob', request),
  getApprovalDetails: (request: P04MainParams['approval.get_details']) =>
    invokeP04('approval.get_details', request),
  getRuntimeReadiness: async () => {
    const result: unknown = await invokeRegistered('runtime.readiness', {});
    if (!isRecord(result) || result.ok !== true) {
      return {
        ok: false as const,
        error:
          isRecord(result) && isIpcErrorEnvelope(result.error)
            ? result.error
            : preloadError('readiness_unavailable', 'Aura Core 的就绪状态暂时读不到，请稍后重试。'),
      };
    }
    try {
      return { ok: true as const, value: parseRuntimeReadiness(result.value) };
    } catch {
      return {
        ok: false as const,
        error: preloadError('invalid_readiness', 'Main returned invalid runtime readiness'),
      };
    }
  },
  getCoreHealth: async (): Promise<ResultEnvelope<CoreHealthProjectionChange>> => {
    const result = (await ipcRenderer.invoke(INVOKE_CHANNEL, {
      command: 'core.health.get',
      params: {},
    })) as unknown;
    return parseProjectionResult(result, 'core_health');
  },
  getHostPreparationStatus: () => invokeP04('security.host_preparation.status', {}),
  getToolCredentialCandidates: (request: P07ReadParams['tools.credential.candidates']) =>
    invokeToolCredentialCandidates(request),
  getModelCompatibility: (request: P05MainParams['models.compatibility.get']) =>
    invokeP05('models.compatibility.get', request),
  getModelProviderCatalog: () => invokeP05('models.catalog.get', {}),
  ensureModelProviderCatalogFresh: () => invokeP05('models.catalog.ensure_fresh', {}),
  getModelSettings: () => invokeP05('models.settings.get_snapshot', {}),
  getPermissionDefaults: () => invokeP04('permission.get_defaults', {}),
  setPermissionDefaults: (request: P04MainParams['permission.set_defaults']) =>
    invokeP04('permission.set_defaults', request),
  getSessionPermissionState: (request: P04MainParams['permission.get_session_state']) =>
    invokeP04('permission.get_session_state', request),
  getWorkspaceSessions: async (): Promise<ResultEnvelope<WorkspaceSessionProjectionChange>> => {
    const result = (await ipcRenderer.invoke(INVOKE_CHANNEL, {
      command: 'workspace_session.get',
      params: {},
    })) as unknown;
    return parseProjectionResult(result, 'workspace_session');
  },
  getWorkspaceFiles: async (): Promise<ResultEnvelope<WorkspaceFilesProjectionChange>> => {
    const result = (await ipcRenderer.invoke(INVOKE_CHANNEL, {
      command: 'workspace_files.get',
      params: {},
    })) as unknown;
    return parseProjectionResult(result, 'workspace_files');
  },
  hashWorkspaceFile: (request: P03MainParams['fs.hash']) => invokeP03('fs.hash', request),
  inspectWorkspaceFile: (request: P03MainParams['fs.inspect']) => invokeP03('fs.inspect', request),
  issueWorkspaceDiscardConfirmation: (
    request: P03MainParams['workspace.lifecycle.issue_discard_confirmation'],
  ) => invokeP03('workspace.lifecycle.issue_discard_confirmation', request),
  listPendingApprovals: () => invokeP04('approval.list_pending', {}),
  listPermissionGrants: (request: P04MainParams['permission.list_grants']) =>
    invokeP04('permission.list_grants', request),
  listExternalGrants: (request: P04MainParams['security.external_grant.list']) =>
    invokeP04('security.external_grant.list', request),
  listQuarantine: () => invokeP04('security.quarantine.list', {}),
  listWorkspaceFiles: (request: P03MainParams['fs.list']) => invokeP03('fs.list', request),
  moveWorkspacePath: (request: P03MainParams['fs.move']) => invokeP03('fs.move', request),
  onModelProjection: (listener: (change: ModelProjectionChange) => void): (() => void) => {
    if (typeof listener !== 'function') throw new TypeError('Model listener must be a function');
    const wrapped = (_event: IpcRendererEvent, value: unknown): void => {
      const change = parseProjection(value);
      if (change !== undefined && isModelProjectionKind(change.projection_kind)) {
        listener(change as ModelProjectionChange);
      }
    };
    ipcRenderer.on(MODEL_PROJECTION_EVENT_CHANNEL, wrapped);
    return () => ipcRenderer.removeListener(MODEL_PROJECTION_EVENT_CHANNEL, wrapped);
  },
  onToolProjection: (listener: (change: ToolProjectionChange) => void): (() => void) => {
    if (typeof listener !== 'function') throw new TypeError('Tool listener must be a function');
    const wrapped = (_event: IpcRendererEvent, value: unknown): void => {
      const change = parseProjection(value);
      if (change !== undefined && isToolProjectionKind(change.projection_kind)) {
        listener(change as ToolProjectionChange);
      }
    };
    ipcRenderer.on(TOOL_PROJECTION_EVENT_CHANNEL, wrapped);
    return () => ipcRenderer.removeListener(TOOL_PROJECTION_EVENT_CHANNEL, wrapped);
  },
  refreshToolProjection: (request: ToolProjectionRouteRequest) =>
    invokeToolProjectionSnapshot(request),
  readToolActivityPage: (request: ToolActivityPageRequestV1) => invokeToolActivityPage(request),
  subscribeModelProjection: (request: ModelProjectionRouteRequest) =>
    invokeModelProjectionRoute('projection.subscribe', request),
  subscribeToolProjection: (request: ToolProjectionRouteRequest) =>
    invokeToolProjectionRoute('projection.subscribe', request),
  unsubscribeModelProjection: (request: ModelProjectionRouteRequest) =>
    invokeModelProjectionRoute('projection.unsubscribe', request),
  unsubscribeToolProjection: (request: ToolProjectionRouteRequest) =>
    invokeToolProjectionRoute('projection.unsubscribe', request),
  onSecurityControl: (
    listener: (change: SecurityControlProjectionChange) => void,
  ): (() => void) => {
    if (typeof listener !== 'function') {
      throw new TypeError('Security Control listener must be a function');
    }
    const wrapped = (_event: IpcRendererEvent, value: unknown): void => {
      const change = parseSecurityProjection(value);
      if (change !== undefined) listener(change);
    };
    ipcRenderer.on(SECURITY_CONTROL_EVENT_CHANNEL, wrapped);
    return () => ipcRenderer.removeListener(SECURITY_CONTROL_EVENT_CHANNEL, wrapped);
  },
  onWorkspaceLifecyclePrepare: (
    listener: (prepare: PrepareEditorLifecycleTransition) => void,
  ): (() => void) => {
    if (typeof listener !== 'function') {
      throw new TypeError('Workspace lifecycle listener must be a function');
    }
    const wrapped = (_event: IpcRendererEvent, value: unknown): void => {
      if (isPrepareEditorLifecycleTransition(value)) listener(value);
    };
    ipcRenderer.on(WORKSPACE_LIFECYCLE_PREPARE_EVENT_CHANNEL, wrapped);
    return () => ipcRenderer.removeListener(WORKSPACE_LIFECYCLE_PREPARE_EVENT_CHANNEL, wrapped);
  },
  openWorkspaceBuffer: (request: P03MainParams['workspace.buffer.open']) =>
    invokeP03('workspace.buffer.open', request),
  openBinaryStream: async (
    request: BinaryStreamOpenRequest,
  ): Promise<ResultEnvelope<AuraBinaryStream>> => {
    if (
      !isRecord(request) ||
      !isCanonicalStreamId(request.streamId) ||
      Object.keys(request).length !== 1
    ) {
      return { error: preloadError('invalid_stream_id', 'Invalid Aura stream ID'), ok: false };
    }
    streamRequestSequence += 1;
    const requestId = `preload-stream-${String(streamRequestSequence)}`;
    return new Promise<ResultEnvelope<AuraBinaryStream>>((resolve, reject) => {
      const timeout = setTimeout(() => {
        pendingStreams.delete(requestId);
        resolve({
          error: preloadError('stream_open_timeout', 'Stream MessagePort timed out'),
          ok: false,
        });
      }, 5_000);
      pendingStreams.set(requestId, { reject, resolve, timeout });
      ipcRenderer.send(STREAM_OPEN_CHANNEL, { requestId, stream: { streamId: request.streamId } });
    });
  },
  readWorkspaceBinaryRange: (request: P03MainParams['fs.read_binary_range']) =>
    invokeP03('fs.read_binary_range', request),
  readWorkspaceText: (request: P03MainParams['fs.read_text']) => invokeP03('fs.read_text', request),
  refreshModelProviderCatalog: () => invokeP05('models.catalog.refresh', {}),
  renameModelProvider: (request: P05MainParams['models.provider.rename']) =>
    invokeP05('models.provider.rename', request),
  setModelProviderConnections: (request: P05MainParams['models.provider.set_connections']) =>
    invokeP05('models.provider.set_connections', request),
  replaceModelCredential: (
    request: P05MainParams['models.endpoint.replace_credential'],
    secret: Uint8Array,
  ) => invokeP05Secret('models.endpoint.replace_credential', request, secret),
  resetModelCompatibility: (request: P05MainParams['models.compatibility.reset']) =>
    invokeP05('models.compatibility.reset', request),
  resetModelRuntimeContextWindow: (
    request: P05MainParams['models.context_window.reset_runtime_correction'],
  ) => invokeP05('models.context_window.reset_runtime_correction', request),
  prepareHostPreparation: (request: P04MainParams['security.host_preparation.prepare']) =>
    invokeP04('security.host_preparation.prepare', request),
  prepareQuarantineRelease: (request: P04MainParams['security.quarantine.prepare_release']) =>
    invokeP04('security.quarantine.prepare_release', request),
  relocateWorkspace: (request: P03MainParams['workspace.relocate']) =>
    invokeP03('workspace.relocate', request),
  resolveWorkspaceLifecycle: (request: P03MainParams['workspace.lifecycle.resolve']) =>
    invokeP03('workspace.lifecycle.resolve', request),
  restoreWorkspace: (request: P03MainParams['workspace.restore']) =>
    invokeP03('workspace.restore', request),
  saveWorkspaceBuffer: (request: P03MainParams['workspace.buffer.save']) =>
    invokeP03('workspace.buffer.save', request),
  resolveApproval: (request: P04MainParams['approval.resolve']) =>
    invokeP04('approval.resolve', request),
  removeHostPreparation: (request: P04MainParams['security.host_preparation.remove']) =>
    invokeP04('security.host_preparation.remove', request),
  repairHostPreparation: (request: P04MainParams['security.host_preparation.repair']) =>
    invokeP04('security.host_preparation.repair', request),
  releaseQuarantine: (request: P04MainParams['security.quarantine.release']) =>
    invokeP04('security.quarantine.release', request),
  revalidateExternalGrant: (request: P04MainParams['security.external_grant.revalidate']) =>
    invokeP04('security.external_grant.revalidate', request),
  revokeExternalGrant: (request: P04MainParams['security.external_grant.revoke']) =>
    invokeP04('security.external_grant.revoke', request),
  revokePermissionGrant: (request: P04MainParams['permission.revoke_grant']) =>
    invokeP04('permission.revoke_grant', request),
  issueQuarantineDiscardConfirmation: (
    request: P04MainParams['security.quarantine.issue_discard_confirmation'],
  ) => invokeP04('security.quarantine.issue_discard_confirmation', request),
  searchWorkspaceText: (request: P03MainParams['fs.search_text']) =>
    invokeP03('fs.search_text', request),
  statWorkspaceFile: (request: P03MainParams['fs.stat']) => invokeP03('fs.stat', request),
  setSessionPermissionMode: (request: P04MainParams['permission.set_session_mode']) =>
    invokeP04('permission.set_session_mode', request),
  setDefaultModel: (request: P05MainParams['models.default.set']) =>
    invokeP05('models.default.set', request),
  setModelProfileSubagentAvailability: (
    request: P05MainParams['models.profile.set_subagent_availability'],
  ) => invokeP05('models.profile.set_subagent_availability', request),
  setProviderModelsSubagentAvailability: (
    request: P05MainParams['models.provider.set_all_current_models_subagent_availability'],
  ) => invokeP05('models.provider.set_all_current_models_subagent_availability', request),
  setSessionModelBinding: (request: P05MainParams['models.session.set_binding']) =>
    invokeP05('models.session.set_binding', request),
  testModelAvailability: (request: P05MainParams['models.profile.test_availability']) =>
    invokeP05('models.profile.test_availability', request),
  updateModelEndpoint: (request: P05MainParams['models.endpoint.update']) =>
    invokeP05('models.endpoint.update', request),
  updateModelProfile: (request: P05MainParams['models.profile.update']) =>
    invokeP05('models.profile.update', request),
  writeWorkspaceFile: (request: P03MainParams['fs.write_file']) =>
    invokeP03('fs.write_file', request),
});

contextBridge.exposeInMainWorld('aura', Object.freeze({ v1: api }));

const invokeModelProjectionRoute = async (
  command: 'projection.subscribe' | 'projection.unsubscribe',
  request: ModelProjectionRouteRequest,
): Promise<ResultEnvelope<void>> => {
  if (!isModelProjectionRouteRequest(request)) {
    return {
      error: preloadError('invalid_projection_route', 'Invalid Model Projection route'),
      ok: false,
    };
  }
  const result = (await invokeRegistered(command, request)) as unknown;
  if (!isRecord(result) || typeof result.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (result.ok === false && isIpcErrorEnvelope(result.error)) {
    return { error: result.error, ok: false };
  }
  if (result.ok === true) return { ok: true, value: undefined };
  return {
    error: preloadError('invalid_result_envelope', 'Main returned an invalid Projection result'),
    ok: false,
  };
};

const invokeToolProjectionRoute = async (
  command: 'projection.subscribe' | 'projection.unsubscribe',
  request: ToolProjectionRouteRequest,
): Promise<ResultEnvelope<void>> => {
  if (!isToolProjectionRouteRequest(request)) {
    return {
      error: preloadError('invalid_projection_route', 'Invalid Tool Projection route'),
      ok: false,
    };
  }
  const result = (await invokeRegistered(command, request)) as unknown;
  if (!isRecord(result) || typeof result.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (result.ok === false && isIpcErrorEnvelope(result.error)) {
    return { error: result.error, ok: false };
  }
  if (result.ok === true) return { ok: true, value: undefined };
  return {
    error: preloadError('invalid_result_envelope', 'Main returned an invalid Projection result'),
    ok: false,
  };
};

const invokeToolProjectionSnapshot = async (
  request: ToolProjectionRouteRequest,
): Promise<ResultEnvelope<ToolProjectionChange>> => {
  if (!isToolProjectionRouteRequest(request)) {
    return {
      error: preloadError('invalid_projection_route', 'Invalid Tool Projection route'),
      ok: false,
    };
  }
  const result = (await ipcRenderer.invoke(INVOKE_CHANNEL, {
    command: 'projection.snapshot',
    params: request,
  })) as unknown;
  if (!isRecord(result) || typeof result.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (result.ok === false && isIpcErrorEnvelope(result.error)) {
    return { error: result.error, ok: false };
  }
  const projection = parseProjection(result.value);
  if (
    result.ok !== true ||
    projection === undefined ||
    !isToolProjectionKind(projection.projection_kind) ||
    toolProjectionRouteKey({
      projection_kind: projection.projection_kind,
      projection_scope: projection.projection_scope,
    } as ToolProjectionRouteRequest) !== toolProjectionRouteKey(request)
  ) {
    return {
      error: preloadError('invalid_projection', 'Main returned an invalid Tool Projection'),
      ok: false,
    };
  }
  return { ok: true, value: projection as ToolProjectionChange };
};

const invokeToolActivityPage = async (
  request: ToolActivityPageRequestV1,
): Promise<ResultEnvelope<ToolActivityPageV1>> => {
  if (!isToolActivityPageRequest(request)) {
    return {
      error: preloadError('invalid_tool_activity_request', 'Invalid Tool Activity request'),
      ok: false,
    };
  }
  const result = (await ipcRenderer.invoke(INVOKE_CHANNEL, {
    command: 'tools.activity.page',
    params: request,
  })) as unknown;
  if (!isRecord(result) || typeof result.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (result.ok === false && isIpcErrorEnvelope(result.error)) {
    return { error: result.error, ok: false };
  }
  if (result.ok === true && isToolActivityPage(result.value)) {
    return { ok: true, value: result.value };
  }
  return {
    error: preloadError(
      'invalid_tool_activity_page',
      'Main returned an invalid Tool Activity page',
    ),
    ok: false,
  };
};

const invokeToolCredentialCandidates = async (
  request: P07ReadParams['tools.credential.candidates'],
): Promise<ResultEnvelope<ToolCredentialCandidatesProjectionV1>> => {
  if (!isP07ReadParams('tools.credential.candidates', request)) {
    return {
      error: preloadError('invalid_p07_request', 'Invalid Tool Credential candidate query'),
      ok: false,
    };
  }
  const result = (await ipcRenderer.invoke(INVOKE_CHANNEL, {
    command: 'tools.credential.candidates',
    params: request,
  })) as unknown;
  if (!isRecord(result) || typeof result.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (result.ok === false && isIpcErrorEnvelope(result.error)) {
    return { error: result.error, ok: false };
  }
  if (result.ok === true && isToolCredentialCandidatesProjection(result.value)) {
    return { ok: true, value: result.value };
  }
  return {
    error: preloadError(
      'invalid_p07_result',
      'Main returned an invalid Tool Credential candidate projection',
    ),
    ok: false,
  };
};

const invokeP03 = async <Command extends P03MainCommand>(
  command: Command,
  request: P03MainParams[Command],
): Promise<ResultEnvelope<P03MainResult[Command]>> => {
  if (!isP03MainParams(command, request)) {
    return {
      error: preloadError('invalid_p03_request', `Invalid parameters for ${command}`),
      ok: false,
    };
  }
  const result = (await invokeRegistered(command, request)) as unknown;
  if (
    (command === 'session.create' ||
      command === 'session.rename' ||
      command === 'workspace.select_directory') &&
    isRecord(result) &&
    result.ok === true &&
    !validateAgentResponse({ command, value: result.value })
  ) {
    return {
      ok: false,
      error: preloadError(
        command === 'session.create' ? 'invalid_session_result' : 'invalid_directory_result',
        `Main returned an invalid result for ${command}`,
      ),
    };
  }
  return parseP03Result<Command>(result);
};

const invokeP04 = async <Command extends P04MainCommand>(
  command: Command,
  request: P04MainParams[Command],
): Promise<ResultEnvelope<P04MainResult[Command]>> => {
  if (!isP04MainParams(command, request)) {
    return {
      error: preloadError('invalid_p04_request', `Invalid parameters for ${command}`),
      ok: false,
    };
  }
  const result = (await invokeRegistered(command, request)) as unknown;
  return parseP04Result<Command>(result);
};

const invokeP05 = async <Command extends P05NonSecretMainCommand>(
  command: Command,
  request: P05MainParams[Command],
): Promise<ResultEnvelope<P05MainResult[Command]>> => {
  if (!isP05MainParams(command, request)) {
    return {
      error: preloadError('invalid_p05_request', `Invalid parameters for ${command}`),
      ok: false,
    };
  }
  const result = (await invokeRegistered(command, request)) as unknown;
  return parseP05Result<Command>(result);
};

const invokeP07 = async <Command extends P07MainCommand>(
  command: Command,
  request: P07MainParams[Command],
): Promise<ResultEnvelope<P07MainResult[Command]>> => {
  if (!isP07MainParams(command, request)) {
    return {
      error: preloadError('invalid_p07_request', `Invalid parameters for ${command}`),
      ok: false,
    };
  }
  const result = (await invokeRegistered(command, request)) as unknown;
  return parseP07Result(command, result);
};

const invokeP05Secret = async <Command extends P05SecretMainCommand>(
  command: Command,
  request: P05MainParams[Command],
  rendererSecret: Uint8Array,
): Promise<ResultEnvelope<P05MainResult[Command]>> => {
  if (!isP05MainParams(command, request) || !isValidApiKeyBytes(rendererSecret)) {
    if (rendererSecret instanceof Uint8Array) rendererSecret.fill(0);
    return {
      error: preloadError('invalid_secret_input', 'Invalid model credential input.'),
      ok: false,
    };
  }
  const preloadSecret = new Uint8Array(rendererSecret);
  rendererSecret.fill(0);
  try {
    const result = (await ipcRenderer.invoke(
      MODEL_SECRET_INVOKE_CHANNEL,
      { command, params: request },
      preloadSecret,
    )) as unknown;
    return parseP05Result<Command>(result);
  } finally {
    preloadSecret.fill(0);
    rendererSecret.fill(0);
  }
};

const parseP05Result = <Command extends P05MainCommand>(
  value: unknown,
): ResultEnvelope<P05MainResult[Command]> => {
  if (!isRecord(value) || typeof value.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (value.ok === false && isIpcErrorEnvelope(value.error)) {
    return { error: value.error, ok: false };
  }
  if (value.ok === true && Object.hasOwn(value, 'value')) {
    return { ok: true, value: value.value as P05MainResult[Command] };
  }
  return {
    error: preloadError('invalid_result_envelope', 'Main returned an invalid P05 result'),
    ok: false,
  };
};

const parseP07Result = <Command extends P07MainCommand>(
  command: Command,
  value: unknown,
): ResultEnvelope<P07MainResult[Command]> => {
  if (!isRecord(value) || typeof value.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (value.ok === false && isIpcErrorEnvelope(value.error)) {
    return { error: value.error, ok: false };
  }
  if (value.ok === true && isToolMutationResult(value.value)) {
    return { ok: true, value: value.value as P07MainResult[Command] };
  }
  return {
    error: preloadError('invalid_p07_result', 'Main returned an invalid Tool mutation result'),
    ok: false,
  };
};

const parseP04Result = <Command extends P04MainCommand>(
  value: unknown,
): ResultEnvelope<P04MainResult[Command]> => {
  if (!isRecord(value) || typeof value.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (value.ok === false && isIpcErrorEnvelope(value.error)) {
    return { error: value.error, ok: false };
  }
  if (value.ok === true && Object.hasOwn(value, 'value')) {
    return { ok: true, value: value.value as P04MainResult[Command] };
  }
  return {
    error: preloadError('invalid_result_envelope', 'Main returned an invalid P04 result'),
    ok: false,
  };
};

const parseP03Result = <Command extends P03MainCommand>(
  value: unknown,
): ResultEnvelope<P03MainResult[Command]> => {
  if (!isRecord(value) || typeof value.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (value.ok === false && isIpcErrorEnvelope(value.error)) {
    return { error: value.error, ok: false };
  }
  if (value.ok === true && Object.hasOwn(value, 'value')) {
    return { ok: true, value: value.value as P03MainResult[Command] };
  }
  return {
    error: preloadError('invalid_result_envelope', 'Main returned an invalid P03 result'),
    ok: false,
  };
};

const wrapPort = (port: MessagePort): AuraBinaryStream => {
  const listeners = new Set<(chunk: BinaryStreamChunk) => void>();
  let settled = false;
  let settleClosed: (result: ResultEnvelope<void>) => void = () => undefined;
  const closed = new Promise<ResultEnvelope<void>>((resolve) => {
    settleClosed = resolve;
  });
  const settle = (result: ResultEnvelope<void>): void => {
    if (settled) return;
    settled = true;
    settleClosed(result);
    listeners.clear();
    port.close();
  };
  port.onmessage = (event: MessageEvent<unknown>) => {
    const value = event.data;
    if (!isRecord(value) || typeof value.type !== 'string') {
      settle({
        error: preloadError('invalid_stream_message', 'Invalid MessagePort payload'),
        ok: false,
      });
      return;
    }
    if (
      value.type === 'chunk' &&
      typeof value.chunkSequence === 'bigint' &&
      typeof value.endOfMessage === 'boolean' &&
      value.data instanceof ArrayBuffer
    ) {
      const chunk: BinaryStreamChunk = {
        chunkSequence: value.chunkSequence,
        data: value.data,
        endOfMessage: value.endOfMessage,
      };
      for (const listener of listeners) listener(chunk);
      return;
    }
    if (value.type === 'closed') settle({ ok: true, value: undefined });
    else if (value.type === 'error' && isIpcErrorEnvelope(value.error)) {
      settle({ error: value.error, ok: false });
    } else {
      settle({
        error: preloadError('invalid_stream_message', 'Invalid MessagePort terminal'),
        ok: false,
      });
    }
  };
  port.onmessageerror = () => {
    settle({
      error: preloadError('stream_message_error', 'MessagePort decoding failed'),
      ok: false,
    });
  };
  port.start();
  return Object.freeze({
    cancel: (): void => {
      if (!settled) port.postMessage({ type: 'cancel' });
    },
    closed,
    onChunk: (listener: (chunk: BinaryStreamChunk) => void): (() => void) => {
      if (typeof listener !== 'function') throw new TypeError('Stream listener must be a function');
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  });
};

const parseProjectionResult = <Kind extends ProjectionChange['projection_kind']>(
  value: unknown,
  expectedKind: Kind,
): ResultEnvelope<Extract<ProjectionChange, { projection_kind: Kind }>> => {
  if (!isRecord(value) || typeof value.ok !== 'boolean') {
    return {
      error: preloadError('invalid_result_envelope', 'Main returned an invalid Result envelope'),
      ok: false,
    };
  }
  if (value.ok === false && isIpcErrorEnvelope(value.error))
    return { error: value.error, ok: false };
  const projection = parseProjection(value.value);
  return projection === undefined || projection.projection_kind !== expectedKind
    ? {
        error: preloadError('invalid_projection', 'Main returned an invalid projection'),
        ok: false,
      }
    : {
        ok: true,
        value: projection as Extract<ProjectionChange, { projection_kind: Kind }>,
      };
};

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isCanonicalStreamId = (value: unknown): value is string =>
  typeof value === 'string' && /^str_[0-7][0-9A-HJKMNP-TV-Z]{25}$/u.test(value);
export {};

async function invokeRegistered(command: string, params: unknown): Promise<unknown> {
  const route = mainRoute(command);
  if (route?.auth !== 'trusted_renderer')
    throw new Error('Unregistered Aura route or incorrect channel');
  return ipcRenderer.invoke(INVOKE_CHANNEL, { command, params });
}

async function invokeRecovery<Command extends ConfigRecoveryMainCommand>(
  command: Command,
  params: ConfigRecoveryMainParams[Command],
): Promise<ResultEnvelope<ConfigRecoveryMainResult[Command]>> {
  if (!isRecoveryParams(command, params))
    return {
      ok: false,
      error: preloadError('invalid_recovery_request', 'Invalid configuration recovery request'),
    };
  const result: unknown = await invokeRegistered(command, params);
  if (isRecord(result) && result.ok === false && isIpcErrorEnvelope(result.error))
    return { ok: false, error: result.error };
  if (isRecord(result) && result.ok === true) {
    try {
      return { ok: true, value: parseRecoveryResult(command, result.value) };
    } catch {
      /* typed failure below */
    }
  }
  return {
    ok: false,
    error:
      command === 'config.recovery.apply'
        ? {
            ...preloadError('invalid_recovery_response', 'Recovery result could not be confirmed'),
            code: 'command.outcome_unknown',
            details: { outcome_recovery: 'query_runtime.command_outcome_without_replay' },
          }
        : preloadError('invalid_recovery_response', 'Invalid configuration recovery response'),
  };
}
