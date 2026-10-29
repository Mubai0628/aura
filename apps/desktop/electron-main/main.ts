import { CORE_NOT_READY_MESSAGE, READINESS_UNAVAILABLE_MESSAGE } from './core-unavailable.js';
import { registerSecretInputIpc } from './secret-ipc.js';
import { registerTerminalOutput } from './terminal-output.js';
import { registerTerminalOpeningIpc } from './terminal-opening-ipc.js';
import { publishToRenderer } from './publish-to-renderer.js';
import { liveContents } from './live-window.js';
import { installMainDiagnostics } from './main-diagnostics.js';
import {
  publishProjectionCompatibility,
  registerProjectionCompatibilityIpc,
} from './projection-compatibility-ipc.js';
import {
  publishScopedModelProjection,
  publishScopedToolProjection,
} from './scoped-projection-publication.js';
import {
  isToolCredentialCommand,
  isToolCredentialParams,
} from '../shared/tool-credential-validation.js';
import { isRecoveryCommand, isRecoveryParams } from '../shared/recovery-validation.js';
import type {
  BeginWorkspaceLifecycleResult,
  IpcErrorEnvelope,
  ToolActivityPageRequestV1,
} from '@aura/contracts/generated';
import {
  app,
  BrowserWindow,
  ipcMain,
  Menu,
  MessageChannelMain,
  net,
  powerMonitor,
  protocol,
  screen,
  session,
  type IpcMainEvent,
  type IpcMainInvokeEvent,
  type MessagePortMain,
} from 'electron';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { mainRoute } from '../shared/generated/routes.js';
import { dispatchDomainRoute, DomainRouteValidationError } from './domain-route.js';
import { dispatchGitCommand } from './git-requests.js';
import { dispatchPackageCommand } from './package-requests.js';
import { dispatchMemoryCommand } from './memory-requests.js';
import { agentRouteHandler } from './command-directory.js';
import { refreshLoadedChromeExtension } from './chrome-extension-folder.js';
import { installBrowserHost, type BrowserHost } from './browser-host/index.js';

import type {
  CoreHealthProjectionChange,
  ModelProjectionChange,
  SecurityControlProjectionChange,
  ToolProjectionChange,
  WorkspaceFilesProjectionChange,
  WorkspaceSessionProjectionChange,
} from '../shared/aura-api.js';

import {
  isP05MainParams,
  isP05SecretMainCommand,
  isValidApiKeyBytes,
} from '../shared/p05-validation.js';
import { isP07ReadParams, type P07ReadParams } from '../shared/p07-validation.js';
import {
  isModelProjectionRouteRequest,
  isToolProjectionRouteRequest,
  type ModelProjectionRouteRequest,
  type ToolProjectionRouteRequest,
} from '../shared/projection-validation.js';
import { isToolActivityPageRequest } from '../shared/tool-projection-validation.js';
import { CoreSupervisor } from './core-supervisor.js';
import { AttachmentFiles } from './attachment-files.js';
import { installMessagePictures } from './message-pictures.js';
import { bindEmbeddedHosts, createMediaHosts } from './embedded-hosts.js';
import { prepareExitThenStop } from './core-exit.js';
import { errorEnvelope } from './core-transport.js';
import { createAuraId } from './ids.js';
import { loadReleaseContext, type ReleaseContext } from './release.js';
import { keepStorageWritten } from './storage-flush.js';
import { restoreWindowState, WindowStateStore } from './window-state.js';
import { createLauncherReadyReporter, type LauncherReadyReporter } from './launcher-ready.js';
import { registerAppUpdates, type RegisteredAppUpdates } from './app-update/index.js';
import { startNotifications, type RegisteredNotifications } from './notifications/start.js';
import { createMainDispatch } from './main-dispatch.js';
import { createProjectionRoutes } from './main-projection-routes.js';

const INVOKE_CHANNEL = 'aura:v1:invoke';
const CORE_HEALTH_EVENT_CHANNEL = 'aura:v1:core-health';
const WORKSPACE_SESSION_EVENT_CHANNEL = 'aura:v1:workspace-session';
const WORKSPACE_FILES_EVENT_CHANNEL = 'aura:v1:workspace-files';
const SECURITY_CONTROL_EVENT_CHANNEL = 'aura:v1:security-control';
const MODEL_SECRET_INVOKE_CHANNEL = 'aura:v1:model-secret';
const STREAM_OPEN_CHANNEL = 'aura:v1:stream-open';
const STREAM_PORT_CHANNEL = 'aura:v1:stream-port';
const PRODUCTION_CSP = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self'",
  "img-src 'self' data: aura:",
  // A workspace's video and sound, streamed from Aura's own scheme.
  "media-src 'self' aura:",
  "font-src 'self'",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "frame-ancestors 'none'",
  "form-action 'none'",
].join('; ');
const APP_CRASH_ESCALATED_EXIT_CODE = 78;
const MAIN_BUILD_EXTENSION_SYMBOL = Symbol.for('aura.main-build-extension.v1');

interface MainBuildExtensionContextV1 {
  assertTrustedSender(event: IpcMainInvokeEvent): void;
  invokeCore(
    method: string,
    params: (identity: { coreRuntimeInstanceId: string; mainRuntimeInstanceId: string }) => unknown,
  ): Promise<unknown>;
}

interface MainBuildExtensionV1 {
  additionalCoreArguments: readonly string[];
  preloadPath: string;
  registerIpc(context: MainBuildExtensionContextV1): void;
}

const mainBuildExtension = Reflect.get(globalThis, MAIN_BUILD_EXTENSION_SYMBOL) as
  MainBuildExtensionV1 | undefined;

protocol.registerSchemesAsPrivileged([
  {
    privileges: { secure: true, standard: true, stream: true, supportFetchAPI: true },
    scheme: 'aura',
  },
]);

const currentDirectory = dirname(fileURLToPath(import.meta.url));
const rendererRoot = resolve(currentDirectory, '../renderer');
const rendererDevelopmentUrl = process.env.AURA_RENDERER_URL;
if (rendererDevelopmentUrl !== undefined && rendererDevelopmentUrl !== 'http://127.0.0.1:5175') {
  throw new Error('AURA_RENDERER_URL must be exactly http://127.0.0.1:5175');
}

const localAppData = process.env.LOCALAPPDATA;
if (localAppData === undefined || localAppData.length === 0) {
  throw new Error('LOCALAPPDATA is required on Windows');
}
const safeMode = process.argv.includes('--safe-mode');
const parentRuntimeInstanceId = process.argv
  .find((argument) => argument.startsWith('--aura-parent-runtime-id='))
  ?.slice('--aura-parent-runtime-id='.length);
const streamPorts = new Set<MessagePortMain>();
const rendererInstances = new Map<number, string>();
const modelProjectionRoutesByWebContents = new Map<number, Set<string>>();
const modelProjectionRoutes = new Map<
  string,
  { owners: Set<number>; route: ModelProjectionRouteRequest }
>();
const toolProjectionRoutesByWebContents = new Map<number, Set<string>>();
const toolProjectionRoutes = new Map<
  string,
  { owners: Set<number>; route: ToolProjectionRouteRequest }
>();
const pendingLifecycleArms = new Map<string, Promise<BeginWorkspaceLifecycleResult>>();
let mainWindow: BrowserWindow | undefined;
const windowContents = () => liveContents(mainWindow);
let releaseContext: ReleaseContext | undefined;
const namespaceDataRoot = (release: ReleaseContext) =>
  resolve(localAppData, 'Aura/namespaces', release.namespaceObjectHash);
let supervisor: CoreSupervisor | undefined;
const { workspaceMedia, mediaHost } = createMediaHosts(() => mainWindow);
// An attached file outside the workspace, for the 文件 tab and its menu.
const attachmentFiles = new AttachmentFiles(workspaceMedia);
let browserHost: BrowserHost | undefined;
let windowStateStore: WindowStateStore | undefined;
let launcherReady: LauncherReadyReporter | undefined;
let appUpdates: RegisteredAppUpdates | undefined;
let notifications: RegisteredNotifications | undefined;
// The launcher starts Aura again when it exits with this code (after an update was applied).
const APP_RESTART_EXIT_CODE = 75;
let exitCode: number | undefined;
let coreExit: Promise<void> | undefined;
const mainDiagnostics = installMainDiagnostics({
  localAppData,
  namespace: () => releaseContext?.namespaceObjectHash,
  contents: windowContents,
  trust: (event) => assertTrustedSender(event),
  flush: () => Promise.all([windowStateStore?.flush(), coreExit]),
  exitCode: () => exitCode,
  stop: () => {
    const active = supervisor;
    supervisor = undefined;
    // Core records a normal exit before it stops (see core-exit.ts).
    coreExit = prepareExitThenStop(active);
  },
});
const appendMainLog = mainDiagnostics.append;
let rendererRecoveryInProgress = false;
let currentProjection = createFallbackProjection(
  'not_started',
  'core.not_started',
  'Aura Core 尚未启动。',
  1,
);
let currentWorkspaceSessionProjection: WorkspaceSessionProjectionChange | undefined;
let currentWorkspaceFilesProjection: WorkspaceFilesProjectionChange | undefined;
const currentSecurityProjections = new Map<
  SecurityControlProjectionChange['projection_kind'],
  SecurityControlProjectionChange
>();

// The projection route owners and the command dispatchers live in their own modules; they
// read `supervisor` and `mainWindow` through these accessors at the points they always did.
const {
  subscribeModelProjectionRoute,
  unsubscribeModelProjectionRoute,
  releaseModelProjectionRoutes,
  subscribeToolProjectionRoute,
  unsubscribeToolProjectionRoute,
  releaseToolProjectionRoutes,
} = createProjectionRoutes({
  supervisor: () => supervisor,
  appendMainLog: (event, fields) => appendMainLog(event, fields),
  modelProjectionRoutesByWebContents,
  modelProjectionRoutes,
  toolProjectionRoutesByWebContents,
  toolProjectionRoutes,
});
const {
  dispatchP05Main,
  dispatchP05SecretMain,
  dispatchP07Main,
  requireActiveAuraWindow,
  dispatchP04Main,
  dispatchP03Main,
  notifyRendererUnavailable,
} = createMainDispatch({
  supervisor: () => supervisor,
  mainWindow: () => mainWindow,
  appendMainLog: (event, fields) => appendMainLog(event, fields),
  rendererInstances,
  pendingLifecycleArms,
});

const registerProductionProtocol = (): void => {
  protocol.handle('aura', async (request) => {
    const url = new URL(request.url);
    if (url.hostname !== 'app') return new Response('Not found', { status: 404 });
    const media = await workspaceMedia.serve(request);
    if (media !== undefined) return media;
    const providerIcon = parseProviderIconPath(url.pathname);
    if (providerIcon !== undefined) {
      if (supervisor === undefined) return new Response('Not found', { status: 404 });
      try {
        const result = await supervisor.requestP05('provider_icon.read', {
          artifact_id: providerIcon.artifactId,
          content_hash: { algorithm: 'blake3', hex: providerIcon.contentHash },
        });
        const bytes = Buffer.from(result.png_base64, 'base64');
        if (
          bytes.byteLength !== result.byte_length ||
          result.content_hash.algorithm !== 'blake3' ||
          result.content_hash.hex !== providerIcon.contentHash ||
          !bytes.subarray(0, 8).equals(Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]))
        ) {
          return new Response('Not found', { status: 404 });
        }
        return new Response(bytes, {
          headers: {
            'Cache-Control': 'private, max-age=86400, immutable',
            'Content-Length': String(bytes.byteLength),
            'Content-Type': 'image/png',
            'X-Content-Type-Options': 'nosniff',
          },
          status: 200,
        });
      } catch {
        return new Response('Not found', { status: 404 });
      }
    }
    if (rendererDevelopmentUrl !== undefined) return new Response('Not found', { status: 404 });
    const decodedPath = decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname);
    const candidate = resolve(rendererRoot, `.${decodedPath}`);
    const back = relative(rendererRoot, candidate);
    if (back.startsWith('..')) return new Response('Not found', { status: 404 });
    const response = await net.fetch(pathToFileURL(candidate).toString());
    const headers = new Headers(response.headers);
    headers.set('Content-Security-Policy', PRODUCTION_CSP);
    headers.set('Cross-Origin-Opener-Policy', 'same-origin');
    headers.set('X-Content-Type-Options', 'nosniff');
    return new Response(response.body, { headers, status: response.status });
  });
};

const parseProviderIconPath = (
  pathname: string,
): { artifactId: `art_${string}`; contentHash: string } | undefined => {
  const match = /^\/provider-icon\/(art_[0-7][0-9A-HJKMNP-TV-Z]{25})\/([0-9a-f]{64})\.png$/u.exec(
    pathname,
  );
  if (match?.[1] === undefined || match[2] === undefined) return undefined;
  return { artifactId: match[1] as `art_${string}`, contentHash: match[2] };
};

const createMainWindow = async (): Promise<BrowserWindow> => {
  const restored = restoreWindowState(
    await windowStateStore?.read(),
    screen.getAllDisplays(),
    screen.getPrimaryDisplay(),
  );
  const window = new BrowserWindow({
    ...restored.bounds,
    backgroundColor: '#f7f8fa',
    minHeight: 540,
    minWidth: 800,
    show: false,
    title: 'Aura',
    webPreferences: {
      contextIsolation: true,
      nodeIntegration: false,
      preload: mainBuildExtension?.preloadPath ?? resolve(currentDirectory, '../preload/index.cjs'),
      sandbox: true,
    },
  });
  mainWindow = window;
  const webContentsId = window.webContents.id;
  rendererInstances.set(webContentsId, createAuraId('rnd'));
  window.webContents.on('did-start-navigation', (details) => {
    if (!details.isMainFrame || details.isSameDocument) return;
    void releaseModelProjectionRoutes(webContentsId);
    const previous = rendererInstances.get(webContentsId);
    rendererInstances.set(webContentsId, createAuraId('rnd'));
    if (previous !== undefined && supervisor !== undefined) {
      void notifyRendererUnavailable(previous);
    }
  });
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', (event, targetUrl) => {
    if (!isAllowedRendererUrl(targetUrl)) event.preventDefault();
  });
  window.webContents.on('did-finish-load', () => {
    window.webContents.send(CORE_HEALTH_EVENT_CHANNEL, structuredClone(currentProjection));
    if (currentWorkspaceSessionProjection !== undefined) {
      window.webContents.send(
        WORKSPACE_SESSION_EVENT_CHANNEL,
        structuredClone(currentWorkspaceSessionProjection),
      );
    }
    if (currentWorkspaceFilesProjection !== undefined) {
      window.webContents.send(
        WORKSPACE_FILES_EVENT_CHANNEL,
        structuredClone(currentWorkspaceFilesProjection),
      );
    }
    for (const change of currentSecurityProjections.values()) {
      window.webContents.send(SECURITY_CONTROL_EVENT_CHANNEL, structuredClone(change));
    }
  });
  window.webContents.on('render-process-gone', (_event, details) => {
    for (const port of streamPorts) port.close();
    streamPorts.clear();
    const rendererInstanceId = rendererInstances.get(webContentsId);
    rendererInstances.delete(webContentsId);
    void releaseModelProjectionRoutes(webContentsId);
    void releaseToolProjectionRoutes(webContentsId);
    if (rendererInstanceId !== undefined && supervisor !== undefined) {
      void notifyRendererUnavailable(rendererInstanceId);
    }
    if (
      window.isDestroyed() ||
      details.reason === 'clean-exit' ||
      mainDiagnostics.isQuitting() ||
      rendererRecoveryInProgress
    )
      return;
    rendererRecoveryInProgress = true;
    windowStateStore?.capture(window, screen.getDisplayMatching(window.getBounds()));
    setTimeout(() => {
      void (windowStateStore?.flush() ?? Promise.resolve())
        .then(() => createMainWindow())
        .then(() => {
          if (!window.isDestroyed()) window.destroy();
        })
        .catch((error: unknown) => {
          if (!window.isDestroyed()) window.webContents.reload();
          void appendMainLog('renderer_rebuild_failed', {
            message: error instanceof Error ? error.message : String(error),
          }).catch(() => undefined);
        })
        .finally(() => {
          rendererRecoveryInProgress = false;
        });
    }, 100);
  });
  const capture = (): void => {
    if (window.isDestroyed()) return;
    windowStateStore?.capture(window, screen.getDisplayMatching(window.getBounds()));
  };
  window.on('move', capture);
  window.on('resize', capture);
  window.on('maximize', capture);
  window.on('unmaximize', capture);
  window.on('enter-full-screen', capture);
  window.on('leave-full-screen', capture);
  window.on('close', capture);
  window.on('closed', () => {
    const rendererInstanceId = rendererInstances.get(webContentsId);
    rendererInstances.delete(webContentsId);
    void releaseModelProjectionRoutes(webContentsId);
    if (
      !mainDiagnostics.isQuitting() &&
      rendererInstanceId !== undefined &&
      supervisor !== undefined
    ) {
      void notifyRendererUnavailable(rendererInstanceId);
    }
  });
  window.once('ready-to-show', () => {
    if (restored.mode === 'maximized') window.maximize();
    if (restored.mode === 'fullscreen') window.setFullScreen(true);
    window.show();
    void launcherReady?.windowShown().catch(() => undefined);
  });

  if (rendererDevelopmentUrl === undefined) await window.loadURL('aura://app/');
  else await window.loadURL(rendererDevelopmentUrl);
  return window;
};

const startSupervisor = async (release: ReleaseContext): Promise<void> => {
  try {
    supervisor = new CoreSupervisor({
      ...(mainBuildExtension === undefined
        ? {}
        : { additionalCoreArguments: mainBuildExtension.additionalCoreArguments }),
      appIsPackaged: app.isPackaged,
      localAppData,
      onEscalated: () => app.exit(APP_CRASH_ESCALATED_EXIT_CODE),
      ...(parentRuntimeInstanceId === undefined ? {} : { parentRuntimeInstanceId }),
      release,
      safeMode,
    });
    registerTerminalOutput(supervisor, windowContents);
    bindEmbeddedHosts(browserHost, mediaHost, supervisor);
    publishProjectionCompatibility(supervisor, windowContents);
    supervisor.subscribe((change) => publishProjection(change));
    supervisor.subscribeWorkspaceSessions((change) => publishWorkspaceSessionProjection(change));
    supervisor.subscribeWorkspaceFiles((change) => publishWorkspaceFilesProjection(change));
    supervisor.subscribeSecurity((change) => publishSecurityProjection(change));
    supervisor.subscribeModels((change) => publishModelProjection(change));
    supervisor.subscribeTools((change) => publishToolProjection(change));
    publishProjection(supervisor.snapshot);
    publishWorkspaceSessionProjection(supervisor.workspaceSessionSnapshot);
    publishWorkspaceFilesProjection(supervisor.workspaceFilesSnapshot);
    await supervisor.start();
    await browserHost?.refreshSettings();
  } catch (error) {
    publishProjection(
      createFallbackProjection(
        'incompatible',
        'release_bundle.invalid',
        error instanceof Error ? error.message : String(error),
        Number(currentProjection.revision) + 1,
      ),
    );
  }
};

const publishProjection = (change: CoreHealthProjectionChange): void => {
  currentProjection = structuredClone(change);
  void launcherReady?.coreState(change.payload.state).catch(() => undefined);
  publishToRenderer(windowContents(), CORE_HEALTH_EVENT_CHANNEL, change);
};

const publishWorkspaceSessionProjection = (change: WorkspaceSessionProjectionChange): void => {
  currentWorkspaceSessionProjection = structuredClone(change);
  publishToRenderer(
    windowContents(),
    WORKSPACE_SESSION_EVENT_CHANNEL,
    currentWorkspaceSessionProjection,
  );
};

const publishWorkspaceFilesProjection = (change: WorkspaceFilesProjectionChange): void => {
  currentWorkspaceFilesProjection = structuredClone(change);
  publishToRenderer(
    windowContents(),
    WORKSPACE_FILES_EVENT_CHANNEL,
    currentWorkspaceFilesProjection,
  );
};

const publishSecurityProjection = (change: SecurityControlProjectionChange): void => {
  currentSecurityProjections.set(change.projection_kind, structuredClone(change));
  publishToRenderer(windowContents(), SECURITY_CONTROL_EVENT_CHANNEL, change);
};

const publishModelProjection = (change: ModelProjectionChange): void => {
  publishScopedModelProjection(windowContents(), change, modelProjectionRoutesByWebContents);
};

const publishToolProjection = (change: ToolProjectionChange): void => {
  if (change.projection_kind === 'tool_settings') {
    appUpdates?.setProxy(change.payload.settings.network_proxy);
  }
  publishScopedToolProjection(windowContents(), change, toolProjectionRoutesByWebContents);
};

const registerIpc = (): void => {
  ipcMain.handle(INVOKE_CHANNEL, async (event, request: unknown) => {
    try {
      assertTrustedSender(event);
      if (!isRecord(request) || typeof request.command !== 'string' || !isRecord(request.params)) {
        return failureResult(
          errorEnvelope('invalid_preload_request', 'Invalid Aura v1 request', 'failed'),
        );
      }
      const route = mainRoute(request.command);
      if (route === undefined || route.auth !== 'trusted_renderer') {
        return failureResult(
          errorEnvelope(
            'invalid_preload_request',
            'Route requires its registered channel.',
            'failed',
          ),
        );
      }
      if (isToolCredentialCommand(request.command)) {
        if (!isToolCredentialParams(request.command, request.params))
          return failureResult(
            errorEnvelope(
              'invalid_preload_request',
              'Invalid tool credential parameters.',
              'failed',
            ),
          );
        if (supervisor === undefined)
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        return {
          ok: true,
          value: await supervisor.requestToolCredential(request.command, request.params),
        };
      }
      if (isRecoveryCommand(request.command)) {
        if (!isRecoveryParams(request.command, request.params))
          return failureResult(
            errorEnvelope(
              'invalid_preload_request',
              'Invalid configuration recovery request',
              'failed',
            ),
          );
        if (supervisor === undefined)
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        return {
          ok: true,
          value: await supervisor.requestConfigRecovery(request.command, request.params),
        };
      }
      if (request.command === 'runtime.readiness') {
        if (!isEmptyRecord(request.params) || supervisor === undefined) {
          return failureResult(
            errorEnvelope('core_not_ready', READINESS_UNAVAILABLE_MESSAGE, 'failed'),
          );
        }
        return { ok: true, value: await supervisor.readRuntimeReadiness() };
      }
      if (request.command === 'core.restart') {
        if (!isEmptyRecord(request.params) || supervisor === undefined)
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        return { ok: true, value: { restarted: supervisor.restartNow() } };
      }
      if (request.command === 'core.health.get') {
        if (!isEmptyRecord(request.params)) {
          return failureResult(
            errorEnvelope('invalid_preload_request', 'Invalid Core Health request', 'failed'),
          );
        }
        const snapshot =
          supervisor === undefined ? currentProjection : await supervisor.refreshSnapshot();
        return { ok: true, value: snapshot };
      }
      if (request.command === 'workspace_session.get') {
        if (!isEmptyRecord(request.params)) {
          return failureResult(
            errorEnvelope('invalid_preload_request', 'Invalid Workspace/Session request', 'failed'),
          );
        }
        if (supervisor !== undefined) {
          return { ok: true, value: await supervisor.refreshWorkspaceSessionSnapshot() };
        }
        if (currentWorkspaceSessionProjection !== undefined) {
          return { ok: true, value: currentWorkspaceSessionProjection };
        }
        return failureResult(
          errorEnvelope(
            'projection_not_ready',
            'Workspace/Session projection is not ready.',
            'failed',
          ),
        );
      }
      if (request.command === 'workspace_files.get') {
        if (!isEmptyRecord(request.params)) {
          return failureResult(
            errorEnvelope('invalid_preload_request', 'Invalid Workspace Files request', 'failed'),
          );
        }
        if (supervisor !== undefined) {
          return { ok: true, value: await supervisor.refreshWorkspaceFilesSnapshot() };
        }
        if (currentWorkspaceFilesProjection !== undefined) {
          return { ok: true, value: currentWorkspaceFilesProjection };
        }
        return failureResult(
          errorEnvelope(
            'projection_not_ready',
            'Workspace Files projection is not ready.',
            'failed',
          ),
        );
      }
      if (
        request.command === 'projection.subscribe' ||
        request.command === 'projection.unsubscribe'
      ) {
        if (
          !isModelProjectionRouteRequest(request.params) &&
          !isToolProjectionRouteRequest(request.params)
        ) {
          return failureResult(
            errorEnvelope('invalid_preload_request', 'Invalid Projection route.', 'failed'),
          );
        }
        if (isToolProjectionRouteRequest(request.params)) {
          if (request.command === 'projection.subscribe') {
            await subscribeToolProjectionRoute(event.sender.id, request.params);
          } else {
            await unsubscribeToolProjectionRoute(event.sender.id, request.params);
          }
        } else if (request.command === 'projection.subscribe') {
          await subscribeModelProjectionRoute(event.sender.id, request.params);
        } else {
          await unsubscribeModelProjectionRoute(event.sender.id, request.params);
        }
        return { ok: true, value: null };
      }
      if (request.command === 'projection.snapshot') {
        if (!isToolProjectionRouteRequest(request.params)) {
          return failureResult(
            errorEnvelope('invalid_preload_request', 'Invalid Tool Projection route.', 'failed'),
          );
        }
        if (supervisor === undefined) {
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        }
        return { ok: true, value: await supervisor.refreshToolProjection(request.params) };
      }
      if (request.command === 'tools.activity.page') {
        if (!isToolActivityPageRequest(request.params)) {
          return failureResult(
            errorEnvelope('invalid_preload_request', 'Invalid Tool Activity request.', 'failed'),
          );
        }
        if (supervisor === undefined) {
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        }
        return {
          ok: true,
          value: await supervisor.readToolActivityPage(request.params as ToolActivityPageRequestV1),
        };
      }
      if (request.command === 'tools.credential.candidates') {
        if (!isP07ReadParams(request.command, request.params)) {
          return failureResult(
            errorEnvelope(
              'invalid_preload_request',
              'Invalid Tool Credential candidate query.',
              'failed',
            ),
          );
        }
        if (supervisor === undefined) {
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        }
        return {
          ok: true,
          value: await supervisor.readToolCredentialCandidates(
            request.params as P07ReadParams['tools.credential.candidates'],
          ),
        };
      }
      if (route.owner !== 'runtime') {
        if (supervisor === undefined)
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        return {
          ok: true,
          value: await dispatchDomainRoute(event, request.command, request.params, {
            workspace: dispatchP03Main,
            git: (command, params) => dispatchGitCommand(command, params, supervisor),
            package: (e, command, params) =>
              dispatchPackageCommand(e, command, params, supervisor, requireActiveAuraWindow),
            memory: (e, command, params) =>
              dispatchMemoryCommand(e, command, params, supervisor, requireActiveAuraWindow),
            security: dispatchP04Main,
            model: dispatchP05Main,
            tool: dispatchP07Main,
            agent: agentRouteHandler(
              supervisor,
              (e) => requireActiveAuraWindow(e, 'Hook folder selection'),
              (snapshot) => browserHost?.applySettings(snapshot),
              () => releaseContext && namespaceDataRoot(releaseContext),
            ),
          }),
        };
      }
      return failureResult(
        errorEnvelope('invalid_preload_request', 'Invalid Aura v1 request', 'failed'),
      );
    } catch (error) {
      const envelope =
        error instanceof Error && 'envelope' in error
          ? (error as { envelope: IpcErrorEnvelope }).envelope
          : errorEnvelope(
              error instanceof DomainRouteValidationError
                ? 'invalid_preload_request'
                : 'main_request_failed',
              error instanceof Error ? error.message : String(error),
              'failed',
            );
      return failureResult(envelope);
    }
  });

  registerSecretInputIpc(() => supervisor, assertTrustedSender);
  registerProjectionCompatibilityIpc(() => supervisor, assertTrustedSender);
  registerTerminalOpeningIpc(() => supervisor, assertTrustedSender);
  workspaceMedia.install(() => supervisor, assertTrustedSender);
  attachmentFiles.install(() => supervisor, assertTrustedSender);
  installMessagePictures(
    () => supervisor,
    () => mainWindow,
    assertTrustedSender,
  );
  browserHost = installBrowserHost({
    window: () => mainWindow,
    assertTrustedSender,
    core: () => supervisor,
    log: (event, fields) => void appendMainLog(event, fields),
  });
  ipcMain.handle(
    MODEL_SECRET_INVOKE_CHANNEL,
    async (event, request: unknown, rendererSecret: unknown) => {
      let mainSecret: Buffer | undefined;
      try {
        assertTrustedSender(event);
        if (
          !isRecord(request) ||
          typeof request.command !== 'string' ||
          !isP05SecretMainCommand(request.command) ||
          !isP05MainParams(request.command, request.params) ||
          !isValidApiKeyBytes(rendererSecret)
        ) {
          if (rendererSecret instanceof Uint8Array) rendererSecret.fill(0);
          return failureResult(
            errorEnvelope('invalid_secret_input', 'Invalid model credential input.', 'failed'),
          );
        }
        mainSecret = Buffer.from(rendererSecret);
        rendererSecret.fill(0);
        if (supervisor === undefined) {
          return failureResult(errorEnvelope('core_not_ready', CORE_NOT_READY_MESSAGE, 'failed'));
        }
        return {
          ok: true,
          value: await dispatchP05SecretMain(request.command, request.params, mainSecret),
        };
      } catch (error) {
        const envelope =
          error instanceof Error && 'envelope' in error
            ? (error as { envelope: IpcErrorEnvelope }).envelope
            : errorEnvelope(
                'model_credential_request_failed',
                'The model credential operation failed.',
                'failed',
              );
        return failureResult(envelope);
      } finally {
        if (rendererSecret instanceof Uint8Array) rendererSecret.fill(0);
        mainSecret?.fill(0);
      }
    },
  );

  ipcMain.on(STREAM_OPEN_CHANNEL, (event, request: unknown) => {
    try {
      assertTrustedSender(event);
      if (
        !isRecord(request) ||
        typeof request.requestId !== 'string' ||
        !isRecord(request.stream) ||
        typeof request.stream.streamId !== 'string' ||
        !/^str_[0-7][0-9A-HJKMNP-TV-Z]{25}$/u.test(request.stream.streamId)
      ) {
        throw new Error('invalid binary stream request');
      }
      const { port1, port2 } = new MessageChannelMain();
      streamPorts.add(port1);
      port1.on('close', () => streamPorts.delete(port1));
      port1.on('message', (messageEvent) => {
        if (isRecord(messageEvent.data) && messageEvent.data.type === 'cancel') port1.close();
      });
      port1.start();
      const senderFrame = event.senderFrame;
      if (senderFrame === null) throw new Error('stream request has no sender frame');
      senderFrame.postMessage(STREAM_PORT_CHANNEL, { requestId: request.requestId }, [port2]);
      setImmediate(() => {
        port1.postMessage({
          error: errorEnvelope(
            'stream_not_open',
            'The requested Core stream is not open.',
            'failed',
          ),
          type: 'error',
        });
        port1.close();
      });
    } catch (error) {
      const requestId =
        isRecord(request) && typeof request.requestId === 'string' ? request.requestId : '';
      const senderFrame = event.senderFrame;
      if (senderFrame === null) return;
      senderFrame.postMessage(STREAM_PORT_CHANNEL, {
        error: errorEnvelope('invalid_stream_request', String(error), 'failed'),
        requestId,
      });
    }
  });
};

const assertTrustedSender = (event: IpcMainInvokeEvent | IpcMainEvent): void => {
  const window = mainWindow;
  if (
    window === undefined ||
    window.isDestroyed() ||
    event.sender.id !== window.webContents.id ||
    event.senderFrame !== event.sender.mainFrame ||
    event.senderFrame !== window.webContents.mainFrame
  ) {
    throw new Error('IPC sender is not the current Aura top-level frame');
  }
  const senderUrl = new URL(event.senderFrame.url);
  const currentUrl = new URL(window.webContents.getURL());
  if (!sameAllowedOrigin(senderUrl, currentUrl) || !isAllowedRendererUrl(senderUrl.toString())) {
    throw new Error('IPC sender origin is not trusted');
  }
};

const sameAllowedOrigin = (left: URL, right: URL): boolean => {
  if (rendererDevelopmentUrl !== undefined)
    return left.origin === right.origin && left.origin === rendererDevelopmentUrl;
  return (
    left.protocol === 'aura:' &&
    right.protocol === 'aura:' &&
    left.hostname === 'app' &&
    right.hostname === 'app'
  );
};

const isAllowedRendererUrl = (value: string): boolean => {
  const url = new URL(value);
  if (rendererDevelopmentUrl !== undefined) return url.origin === rendererDevelopmentUrl;
  return url.protocol === 'aura:' && url.hostname === 'app';
};

function createFallbackProjection(
  state: CoreHealthProjectionChange['payload']['state'],
  statusCode: string,
  statusMessage: string,
  revision: number,
): CoreHealthProjectionChange {
  return {
    payload: {
      read_only: state !== 'ready',
      restart_attempt: 0,
      safe_mode: safeMode,
      state,
      status_code: statusCode,
      status_message: statusMessage,
    },
    projection_kind: 'core_health',
    projection_scope: { type: 'global' },
    revision: String(revision),
    runtime_instance_id: createAuraId('run'),
  };
}

const failureResult = (error: IpcErrorEnvelope): { error: IpcErrorEnvelope; ok: false } => ({
  error,
  ok: false,
});
const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isEmptyRecord = (value: unknown): value is Record<string, never> =>
  isRecord(value) && Object.keys(value).length === 0;

app.whenReady().then(async () => {
  // Electron's default window menu reloads the page and opens the developer tools.
  if (app.isPackaged) Menu.setApplicationMenu(null);
  registerProductionProtocol();
  registerIpc();
  mainBuildExtension?.registerIpc({
    assertTrustedSender(event) {
      assertTrustedSender(event);
    },
    async invokeCore(method, params) {
      if (supervisor === undefined) throw new Error('Aura Core is not ready');
      return supervisor.requestBuildExtension(method, params);
    },
  });
  session.defaultSession.setPermissionRequestHandler((_webContents, _permission, callback) => {
    callback(false);
  });
  keepStorageWritten(session.defaultSession);
  try {
    releaseContext = await loadReleaseContext({
      appIsPackaged: app.isPackaged,
      currentDirectory,
      executablePath: process.execPath,
    });
    windowStateStore = new WindowStateStore(namespaceDataRoot(releaseContext));
    void refreshLoadedChromeExtension(namespaceDataRoot(releaseContext), appendMainLog);
    notifications = startNotifications({
      namespace: releaseContext.manifest.bundle_namespace,
      dataRoot: namespaceDataRoot(releaseContext),
      window: () => mainWindow,
      trust: (event) => assertTrustedSender(event),
      core: () => supervisor,
      log: (event, fields) => void appendMainLog(event, fields),
    });
    appUpdates = registerAppUpdates({
      appIsPackaged: app.isPackaged,
      manifestPath: releaseContext.manifestPath,
      dataRoot: namespaceDataRoot(releaseContext),
      current: releaseContext.manifest.product_version,
      namespace: releaseContext.manifest.bundle_namespace,
      contents: windowContents,
      trust: (event) => assertTrustedSender(event),
      requestRestart: () => {
        exitCode = APP_RESTART_EXIT_CODE;
        app.quit();
      },
      log: (event, fields) => void appendMainLog(event, fields),
      onState: (state) => notifications?.updateState(state),
    });
    if (app.isPackaged) {
      launcherReady = createLauncherReadyReporter({
        dataRoot: namespaceDataRoot(releaseContext),
        releaseBundleId: releaseContext.manifest.release_bundle_id,
        productVersion: releaseContext.manifest.product_version,
      });
    }
    // The bounded record of which Core this run is bound to. Development has to be
    // able to say "the restricted Shell is unready and here is the exact gap"
    // rather than letting a published installation silently fail to match.
    await appendMainLog('core.binding', {
      state: releaseContext.coreBinding.state,
      source: releaseContext.coreBinding.source,
      refusal: releaseContext.coreBinding.refusal,
      mismatched_components: releaseContext.coreBinding.mismatchedComponents,
      unknown_components: releaseContext.coreBinding.unknownComponents ?? [],
      installation_id: releaseContext.coreBinding.installationId,
      core_executable: releaseContext.coreExecutable,
    });
  } catch (error) {
    publishProjection(
      createFallbackProjection(
        'incompatible',
        'release_bundle.invalid',
        error instanceof Error ? error.message : String(error),
        Number(currentProjection.revision) + 1,
      ),
    );
  }
  await createMainWindow();
  powerMonitor.on('suspend', () => supervisor?.onSuspend());
  powerMonitor.on('resume', () => supervisor?.onResume());
  if (releaseContext !== undefined) void startSupervisor(releaseContext);
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) void createMainWindow();
  });
});

app.on('window-all-closed', () => app.quit());
