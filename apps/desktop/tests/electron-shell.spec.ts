import { expect, test } from '@playwright/test';
import { expectSecureSurface } from './electron-shell/renderer-api.js';
import {
  RendererAura,
  CoreTransition,
  launchAura,
  closeAura,
  readMainLog,
  readCoreLog,
} from './electron-shell/harness.js';

test('Launcher target shell reaches Core Ready over the secure Named Pipe', async () => {
  const application = await launchAura();
  try {
    const window = await application.firstWindow();
    await expect(window).toHaveTitle('Aura');
    await expect(window.getByRole('heading', { level: 2, name: '欢迎使用 Aura' })).toBeVisible();
    // With no workspace the page is the new conversation's, with the folder choice and no
    // message box.
    await expect(window.getByRole('button', { name: '选择文件夹创建工作区' })).toBeVisible();
    await expect(window.getByRole('region', { name: '消息编辑器' })).toHaveCount(0);
    await expect(window.locator('.app-topbar')).toHaveCount(0);
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    await expect(window.getByText('本地运行 · 无需登录')).toBeVisible();
    await expectSecureSurface(window);
    const snapshot = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(snapshot.ok).toBe(true);
    if (snapshot.ok) expect(snapshot.value.payload.state).toBe('ready');
    const workspaceSessions = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getWorkspaceSessions();
    });
    expect(workspaceSessions.ok).toBe(true);
    if (workspaceSessions.ok) {
      expect(workspaceSessions.value.projection_kind).toBe('workspace_session');
      expect(workspaceSessions.value.payload).toEqual({ sessions: [], workspaces: [] });
    }
    const workspaceFiles = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getWorkspaceFiles();
    });
    expect(workspaceFiles.ok).toBe(true);
    if (workspaceFiles.ok) {
      expect(workspaceFiles.value.projection_kind).toBe('workspace_files');
      expect(workspaceFiles.value.payload).toEqual({ lifecycle_operations: [], workspaces: [] });
    }
    await expect
      .poll(async () =>
        (await readCoreLog(application)).includes('p05_clean_acceptance_profile_verified'),
      )
      .toBe(true);
  } finally {
    await closeAura(application);
  }
});

test('missing Core renders an unavailable state without exposing Node', async () => {
  const application = await launchAura('missing');
  try {
    const window = await application.firstWindow();
    const currentSession = window.getByRole('region', { name: '会话状态' });
    await expect(currentSession.getByText('无法启动 Aura Core')).toBeVisible({ timeout: 8_000 });
    await expect(window.getByText('未找到或无法启动 aura-core.exe。')).toBeVisible();
    await expectSecureSurface(window);
  } finally {
    await closeAura(application);
  }
});

test('protocol major mismatch blocks the runtime', async () => {
  const application = await launchAura('incompatible');
  try {
    const window = await application.firstWindow();
    const currentSession = window.getByRole('region', { name: '会话状态' });
    await expect(currentSession.getByText('组件版本不兼容')).toBeVisible({ timeout: 10_000 });
    await expect(window.getByText(/IPC 主版本不兼容/u)).toBeVisible();
  } finally {
    await closeAura(application);
  }
});

test('Core crash enters read-only recovery and restarts to Ready', async () => {
  const application = await launchAura('crash-after-handshake');
  try {
    const window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 12_000 });
    await expect
      .poll(async () => {
        const log = await readMainLog(application);
        return (
          log.includes('"event":"core_exited"') &&
          (log.match(/"event":"core_spawned"/gu)?.length ?? 0) >= 2
        );
      })
      .toBe(true);
    const snapshot = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(snapshot.ok).toBe(true);
    if (snapshot.ok) expect(snapshot.value.payload.state).toBe('ready');
  } finally {
    await closeAura(application);
  }
});

test('Projection revision gap reloads the authoritative snapshot', async () => {
  const application = await launchAura('projection-revision-gap');
  try {
    const window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const snapshot = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(snapshot.ok).toBe(true);
    if (snapshot.ok) {
      expect(snapshot.value.payload.state).toBe('ready');
      expect(Number(snapshot.value.revision)).toBeLessThan(4);
    }
  } finally {
    await closeAura(application);
  }
});

test('Named Pipe disconnect reconnects to the same Core instance', async () => {
  const application = await launchAura('disconnect-after-ready');
  try {
    const window = await application.firstWindow();
    await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & {
        aura: RendererAura;
        auraCoreTransitions?: CoreTransition[];
      };
      rendererGlobal.auraCoreTransitions = [];
      rendererGlobal.aura.v1.onCoreHealth((change) => {
        rendererGlobal.auraCoreTransitions?.push({
          runtimeInstanceId: change.runtime_instance_id,
          state: change.payload.state,
        });
      });
    });
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    await expect
      .poll(
        () =>
          window.evaluate(() => {
            const rendererGlobal = globalThis as typeof globalThis & {
              auraCoreTransitions?: CoreTransition[];
            };
            const transitions = rendererGlobal.auraCoreTransitions ?? [];
            const firstReady = transitions.findIndex(({ state }) => state === 'ready');
            const restarting = transitions.findIndex(
              ({ state }, index) => index > firstReady && state === 'restarting',
            );
            return (
              firstReady >= 0 &&
              restarting > firstReady &&
              transitions.some(({ state }, index) => index > restarting && state === 'ready')
            );
          }),
        { timeout: 8_000 },
      )
      .toBe(true);
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 8_000 });
    const after = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(after.ok).toBe(true);
    const transitions = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & {
        auraCoreTransitions?: CoreTransition[];
      };
      return rendererGlobal.auraCoreTransitions ?? [];
    });
    const firstReady = transitions.findIndex(({ state }) => state === 'ready');
    const restarting = transitions.findIndex(
      ({ state }, index) => index > firstReady && state === 'restarting',
    );
    const readyAgain = transitions.findIndex(
      ({ state }, index) => index > restarting && state === 'ready',
    );
    if (after.ok) {
      expect(transitions[firstReady]?.runtimeInstanceId).toBe(after.value.runtime_instance_id);
      expect(transitions[restarting]?.runtimeInstanceId).toBe(after.value.runtime_instance_id);
      expect(transitions[readyAgain]?.runtimeInstanceId).toBe(after.value.runtime_instance_id);
    }
    await expect
      .poll(async () => (await readMainLog(application)).includes('core_transport_reconnected'))
      .toBe(true);
    const mainLog = await readMainLog(application);
    expect(mainLog.match(/"event":"core_spawned"/gu)).toHaveLength(1);
    expect(mainLog.match(/"event":"core_transport_reconnected"/gu)).toHaveLength(1);
    expect(mainLog).toContain('"connectionEpoch":2');
    expect(mainLog).not.toContain('"event":"core_exited"');
  } finally {
    await closeAura(application);
  }
});

test('Renderer crash rebuilds the page without restarting Core', async () => {
  const application = await launchAura();
  try {
    const firstWindow = await application.firstWindow();
    await expect(firstWindow.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const before = await firstWindow.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(before.ok).toBe(true);
    const rebuiltWindowPromise = application.waitForEvent('window', { timeout: 10_000 });
    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.webContents.forcefullyCrashRenderer();
    });
    const rebuiltWindow = await rebuiltWindowPromise;
    await expect(rebuiltWindow.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 10_000 });
    const after = await rebuiltWindow.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(after.ok).toBe(true);
    if (before.ok && after.ok) {
      expect(after.value.runtime_instance_id).toBe(before.value.runtime_instance_id);
    }
  } finally {
    await closeAura(application);
  }
});

test('migration recovery keeps Core alive in diagnostic read-only mode', async () => {
  const application = await launchAura('migration-recovery-required');
  try {
    const window = await application.firstWindow();
    const currentSession = window.getByRole('region', { name: '会话状态' });
    await expect(currentSession.getByText('Aura 已进入诊断模式')).toBeVisible({ timeout: 10_000 });
    // Core's own sentence, in Chinese, says what stopped and what still works.
    await expect(window.getByText(/智能体已停止运行/u)).toBeVisible();
    const health = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getCoreHealth();
    });
    expect(health.ok).toBe(true);
    if (health.ok) {
      expect(health.value.payload.state).toBe('recovery_required');
      expect(health.value.payload.read_only).toBe(true);
    }
    const workspaceSessions = await window.evaluate(() => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.getWorkspaceSessions();
    });
    expect(workspaceSessions.ok).toBe(false);
    await window.waitForTimeout(3_000);
    await expect(currentSession.getByText('Aura 已进入诊断模式')).toBeVisible();
  } finally {
    await closeAura(application);
  }
});
