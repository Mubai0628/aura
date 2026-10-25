import type {
  BeginWorkspaceLifecycleResult,
  PrepareEditorLifecycleTransition,
  ProjectionChange,
  SessionCreateRequest,
  SessionSummary,
  WorkspaceCreateResult,
  WorkspaceLifecycleSummary,
} from '@aura/contracts/generated';
import {
  _electron as electron,
  expect,
  type ElectronApplication,
  type Page,
} from '@playwright/test';
import { mkdir, mkdtemp, readFile, readdir, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

/** Launching, restarting and closing the shell under test, and reading its logs. */
export type CoreHealthProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'core_health' }
>;
export type WorkspaceSessionProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'workspace_session' }
>;
export type WorkspaceFilesProjectionChange = Extract<
  ProjectionChange,
  { projection_kind: 'workspace_files' }
>;

export interface RendererAura {
  readonly v1: {
    acknowledgeWorkspaceLifecycle(request: {
      buffers: [];
      lifecycle_revision: string;
      operation_id: string;
      workspace_id: string;
    }): Promise<RendererResult<WorkspaceLifecycleSummary>>;
    archiveWorkspace(request: {
      expected_workspace_revision: string;
      workspace_id: string;
    }): Promise<RendererResult<BeginWorkspaceLifecycleResult>>;
    createWorkspace(request: {
      display_name: string;
      selected_root: string;
    }): Promise<RendererResult<WorkspaceCreateResult>>;
    createSession(request: SessionCreateRequest): Promise<RendererResult<SessionSummary>>;
    getCoreHealth(): Promise<
      { ok: true; value: CoreHealthProjectionChange } | { error: unknown; ok: false }
    >;
    getWorkspaceSessions(): Promise<
      { ok: true; value: WorkspaceSessionProjectionChange } | { error: unknown; ok: false }
    >;
    getWorkspaceFiles(): Promise<
      { ok: true; value: WorkspaceFilesProjectionChange } | { error: unknown; ok: false }
    >;
    onCoreHealth(listener: (change: CoreHealthProjectionChange) => void): () => void;
    onWorkspaceLifecyclePrepare(
      listener: (prepare: PrepareEditorLifecycleTransition) => void,
    ): () => void;
  };
}

export type RendererResult<Value> =
  { ok: true; value: Value } | { error: { code?: string; message?: string }; ok: false };

export interface CoreTransition {
  runtimeInstanceId: string;
  state: CoreHealthProjectionChange['payload']['state'];
}

const testDataRoots = new WeakMap<ElectronApplication, string>();

export const launchAuraWithDataRoot = async (
  localAppData: string,
  coreMode = 'normal',
  options: {
    entry?: 'e2e' | 'production';
    environment?: Readonly<Record<string, string>>;
  } = {},
): Promise<ElectronApplication> => {
  const entry =
    options.entry === 'production'
      ? resolve(import.meta.dirname, '../..')
      : resolve(import.meta.dirname, '../../dist/e2e/electron-main/main.js');
  const application = await electron.launch({
    args: [`--user-data-dir=${join(localAppData, 'Electron')}`, entry],
    env: {
      ...process.env,
      APPDATA: join(localAppData, 'Roaming'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: coreMode,
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
      LOCALAPPDATA: localAppData,
      AURA_MEMORY_TEST_ROOT: join(localAppData, 'global-memories'),
      ...options.environment,
    },
  });
  testDataRoots.set(application, localAppData);
  return application;
};

export const launchProductionAura = async (): Promise<ElectronApplication> => {
  const localAppData = await mkdtemp(join(tmpdir(), 'aura-electron-production-surface-'));
  try {
    return await launchAuraWithDataRoot(localAppData, 'normal', { entry: 'production' });
  } catch (error) {
    await rm(localAppData, { force: true, recursive: true });
    throw error;
  }
};

export const launchAura = async (
  coreMode = 'normal',
  environment?: Readonly<Record<string, string>>,
): Promise<ElectronApplication> => {
  const localAppData = await mkdtemp(join(tmpdir(), 'aura-electron-e2e-'));
  try {
    return await launchAuraWithDataRoot(
      localAppData,
      coreMode,
      environment === undefined ? {} : { environment },
    );
  } catch (error) {
    await rm(localAppData, { force: true, recursive: true });
    throw error;
  }
};

export const restartAura = async (
  application: ElectronApplication,
): Promise<ElectronApplication> => {
  const localAppData = testDataRoots.get(application);
  if (localAppData === undefined) throw new Error('Electron test data root is unavailable');
  testDataRoots.delete(application);
  await application.close();
  return launchAuraWithDataRoot(localAppData);
};

export const closeAura = async (application: ElectronApplication): Promise<void> => {
  const localAppData = testDataRoots.get(application);
  await application.close();
  if (localAppData !== undefined) {
    testDataRoots.delete(application);
    await rm(localAppData, { force: true, maxRetries: 3, recursive: true, retryDelay: 100 });
  }
};

// Use the production bridge and Core projections, with a workspace owned by this
// test's disposable data root. Fresh profiles contain no demonstration sessions.
export const createShellSession = async (
  application: ElectronApplication,
  window: Page,
): Promise<SessionSummary> => {
  const localAppData = testDataRoots.get(application);
  if (localAppData === undefined) throw new Error('Electron test data root is unavailable');
  const selectedRoot = join(localAppData, 'Workspace');
  await mkdir(selectedRoot, { recursive: true });
  const session = await window.evaluate(async (root) => {
    const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
    const workspace = await rendererGlobal.aura.v1.createWorkspace({
      display_name: 'Shell E2E',
      selected_root: root,
    });
    if (!workspace.ok) throw new Error(JSON.stringify(workspace.error));
    const created = await rendererGlobal.aura.v1.createSession({
      title: 'Core-backed shell session',
      workspace_id: workspace.value.workspace.workspace_id,
    });
    if (!created.ok) throw new Error(JSON.stringify(created.error));
    return created.value;
  }, selectedRoot);
  await expect(window.getByRole('heading', { level: 2, name: session.title })).toBeVisible();
  const projection = await window.evaluate(async () => {
    const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
    return rendererGlobal.aura.v1.getWorkspaceSessions();
  });
  expect(projection.ok).toBe(true);
  if (projection.ok) expect(projection.value.payload.sessions).toContainEqual(session);
  return session;
};

export const readMainLog = async (application: ElectronApplication): Promise<string> => {
  const localAppData = testDataRoots.get(application);
  if (localAppData === undefined) throw new Error('Electron test data root is unavailable');
  const namespacesRoot = join(localAppData, 'Aura', 'namespaces');
  const namespaces = await readdir(namespacesRoot);
  if (namespaces.length !== 1 || namespaces[0] === undefined) {
    throw new Error('Electron test must create exactly one namespace data partition');
  }
  return readFile(join(namespacesRoot, namespaces[0], 'logs', 'main.jsonl'), 'utf8');
};

export const readCoreLog = async (application: ElectronApplication): Promise<string> => {
  const localAppData = testDataRoots.get(application);
  if (localAppData === undefined) throw new Error('Electron test data root is unavailable');
  return readFile(join(localAppData, 'Aura', 'logs', 'core.jsonl'), 'utf8');
};
