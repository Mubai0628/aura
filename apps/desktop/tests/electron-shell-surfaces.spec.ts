import { expect, test } from '@playwright/test';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  RendererAura,
  launchAura,
  closeAura,
  createShellSession,
} from './electron-shell/harness.js';

// eslint-disable-next-line no-empty-pattern -- Playwright requires object destructuring even when an Electron-only test uses no browser fixtures.
test('Core-backed shell exposes honest empty state and navigable tool and model surfaces', async ({}, testInfo) => {
  const application = await launchAura();
  try {
    const window = await application.firstWindow();
    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(1_600, 900);
    });
    const pageErrors: string[] = [];
    const consoleErrors: string[] = [];
    window.on('pageerror', (error) => pageErrors.push(error.message));
    window.on('console', (message) => {
      if (message.type() === 'error') consoleErrors.push(message.text());
    });

    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    await expect(window.getByText('本地运行 · 无需登录')).toBeVisible();
    await expect(window.getByRole('button', { name: '登录' })).toHaveCount(0);
    const session = await createShellSession(application, window);
    const sessionHeading = window.getByRole('heading', { level: 2, name: session.title });
    await expect(window.locator('.app-topbar')).toHaveCount(0);
    await expect(window.locator('.conversation-header p')).toHaveCount(0);
    // Empty projections must stay empty; do not resurrect demonstration messages
    // or Todo items to satisfy the retired mock-shell assertions.
    await expect(window.locator('.message-user, .message-aura, .todo-dock')).toHaveCount(0);

    const contextMeter = window.getByRole('progressbar', { name: '上下文使用情况' });
    await expect(contextMeter).toHaveAttribute('aria-valuenow', '0');
    await expect(contextMeter).toHaveAttribute('aria-valuetext', '本会话尚无模型请求');
    await window.getByRole('button', { name: '上下文占用：本会话尚无模型请求' }).click();
    const contextDetails = window.getByRole('region', { name: '上下文占用详情' });
    await expect(contextDetails.getByRole('note')).toContainText('没有可展示的上下文用量');
    // The slim bar lists no part (the names are for screen readers and the pointer).
    await expect(contextDetails.locator('.sr-only li')).toHaveCount(0);
    const contextScreenshot = testInfo.outputPath('core-context-empty.png');
    await window.screenshot({ path: contextScreenshot });
    await testInfo.attach('Core session with no model request', {
      contentType: 'image/png',
      path: contextScreenshot,
    });
    await sessionHeading.click();
    await expect(contextDetails).toHaveCount(0);

    // The sidebar's "+" no longer offers 提示词监测; `/context` opens it.
    await window.getByRole('region', { name: '消息编辑器' }).getByRole('textbox').click();
    await window.keyboard.type('/context');
    await window.keyboard.press('Enter');
    const rightWorkspace = window.getByRole('complementary', { name: '右侧工作区' });
    await expect(rightWorkspace.getByRole('status')).toContainText('检查此步骤');
    await expect(window.getByText('Aura fixture core system rules.')).toHaveCount(0);
    await window.getByRole('button', { name: '关闭提示词监测' }).click();

    const permissionButton = window
      .getByRole('region', { name: '消息编辑器' })
      .getByRole('button', { name: /^(只读|工作区读写|完全访问)$/u });
    await expect(permissionButton).toBeEnabled();
    await permissionButton.click();
    await expect(window.getByRole('menu', { name: '快速选择权限' })).toContainText('当前会话权限');
    await sessionHeading.click();
    await expect(window.getByRole('menu', { name: '快速选择权限' })).toHaveCount(0);
    await permissionButton.click();
    const permissionMenu = window.getByRole('menu', { name: '快速选择权限' });
    await permissionMenu.getByRole('menuitemradio', { name: '只读' }).click();
    await expect(permissionButton).toHaveText('只读');
    await permissionButton.click();
    await expect(
      window
        .getByRole('menu', { name: '快速选择权限' })
        .getByRole('menuitemradio', { name: '只读' }),
    ).toHaveAttribute('aria-checked', 'true');
    await sessionHeading.click();

    await window.getByRole('button', { name: '选择模型', exact: true }).click();
    const modelMenu = window.getByRole('menu', { name: '快速选择模型' });
    await expect(modelMenu.getByText('尚未添加模型，请前往设置添加。')).toBeVisible();
    await expect(modelMenu.getByRole('group')).toHaveCount(0);
    await expect(modelMenu.getByRole('menuitem', { name: '管理模型' })).toBeVisible();
    await sessionHeading.click();
    await expect(modelMenu).toHaveCount(0);

    // The composer's + adds files, a command or a Skill; Plan and Goal are /plan and /goal.
    await window.getByRole('button', { name: '添加', exact: true }).click();
    const addMenu = window.getByRole('menu', { name: '添加' });
    for (const item of ['图片或文件', '命令', 'Skill']) {
      await expect(addMenu.getByRole('menuitem', { name: item, exact: true })).toBeEnabled();
    }
    await sessionHeading.click();
    await expect(addMenu).toHaveCount(0);

    await window.getByRole('button', { name: '打开工具', exact: true }).click();
    await expect(window.getByRole('menu', { name: '选择工具' })).toBeVisible();
    await sessionHeading.click();
    await expect(window.getByRole('menu', { name: '选择工具' })).toHaveCount(0);

    await window.getByRole('button', { name: '关闭右侧边栏' }).click();
    await expect(rightWorkspace).toHaveCount(0);
    await expect(window.getByRole('button', { name: '展开右侧边栏' })).toBeVisible();
    const activity = window.getByRole('complementary', { name: '近期活动', includeHidden: true });
    const activityEntry = window.getByRole('button', { name: '展开近期活动（当前模式 Normal）' });
    await expect(activityEntry).toBeVisible();
    await expect(activity).toBeHidden();
    await activityEntry.click();
    await expect(activity).toBeVisible();
    // The card names the mode on a row of its own, with what the conversation is doing.
    await expect(activity.locator('.activity-mode-name')).toHaveText('模式Normal');
    await expect(activity).toContainText('状态');
    await window.keyboard.press('Escape');
    await expect(activity).toBeHidden();
    await expect(activityEntry).toBeFocused();

    const viewportBox = await window.locator('.conversation-scroll').boundingBox();
    const columnBox = await window.locator('.conversation-column').boundingBox();
    if (viewportBox === null || columnBox === null) {
      throw new Error('Conversation viewport and column bounds must be measurable');
    }
    expect(
      Math.abs(viewportBox.x + viewportBox.width / 2 - columnBox.x - columnBox.width / 2),
    ).toBeLessThanOrEqual(1);

    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(1_920, 900);
    });
    await expect(activity).toHaveAttribute('data-display', 'full');
    await expect(activity).toBeVisible();
    // The entrance animation starts at scale(0.84); measure the settled card.
    await expect
      .poll(() =>
        activity.evaluate(
          (element) =>
            element
              .getAnimations()
              .filter((animation) => animation.pending || animation.playState === 'running').length,
        ),
      )
      .toBe(0);
    const activityBox = await activity.boundingBox();
    const wideConversationBox = await window.locator('.conversation-column').boundingBox();
    if (activityBox === null || wideConversationBox === null) {
      throw new Error('Activity and conversation bounds must be measurable');
    }
    await testInfo.attach('Activity and conversation bounds after entrance', {
      body: JSON.stringify({ activity: activityBox, conversation: wideConversationBox }),
      contentType: 'application/json',
    });
    expect(activityBox.width).toBeGreaterThanOrEqual(280);
    expect(activityBox.width).toBeLessThanOrEqual(320);
    expect(wideConversationBox.x + wideConversationBox.width).toBeLessThanOrEqual(activityBox.x);
    const activityScreenshot = testInfo.outputPath('core-activity-wide.png');
    await window.screenshot({ path: activityScreenshot });
    await testInfo.attach('Real session mode card without conversation overlap', {
      contentType: 'image/png',
      path: activityScreenshot,
    });

    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(1_600, 900);
    });
    await expect(activityEntry).toBeVisible();
    await expect(activity).toBeHidden();
    await window.getByRole('button', { name: '展开右侧边栏' }).click();
    await expect(rightWorkspace).toBeVisible();

    const rightSeparator = window.getByRole('separator', { name: '调整右侧工作区宽度' });
    const initialRightWidth = Number(await rightSeparator.getAttribute('aria-valuenow'));
    const maximumRightWidth = Number(await rightSeparator.getAttribute('aria-valuemax'));
    await rightSeparator.press('ArrowLeft');
    await expect(rightSeparator).toHaveAttribute(
      'aria-valuenow',
      String(Math.min(maximumRightWidth, initialRightWidth + 16)),
    );

    const composerSeparator = window.getByRole('separator', { name: '调整消息编辑器高度' });
    const initialComposerHeight = Number(await composerSeparator.getAttribute('aria-valuenow'));
    await composerSeparator.press('ArrowUp');
    await expect(composerSeparator).toHaveAttribute(
      'aria-valuenow',
      String(initialComposerHeight + 12),
    );

    await expect(window.locator('.todo-dock')).toHaveCount(0);

    const inspectConversationDock = () =>
      window.evaluate(() => {
        const canvas = document.querySelector<HTMLElement>('.conversation-canvas');
        const scroll = document.querySelector<HTMLElement>('.conversation-scroll');
        const dock = document.querySelector<HTMLElement>('.conversation-dock');
        const composer = document.querySelector<HTMLElement>('.composer');
        if (canvas === null || scroll === null || dock === null || composer === null) {
          throw new Error('Conversation dock surface is incomplete');
        }
        const canvasRect = canvas.getBoundingClientRect();
        const dockRect = dock.getBoundingClientRect();
        const composerRect = composer.getBoundingClientRect();
        const canvasStyle = getComputedStyle(canvas);
        const scrollStyle = getComputedStyle(scroll);
        const dockStyle = getComputedStyle(dock);
        return {
          canvasBackground: canvasStyle.backgroundColor,
          canvasBottom: canvasRect.bottom,
          composerBottom: composerRect.bottom,
          dockBackground: dockStyle.backgroundColor,
          dockBottom: dockRect.bottom,
          dockHeight: dockRect.height,
          dockParentIsCanvas: dock.parentElement === canvas,
          dockPosition: dockStyle.position,
          scrollBackground: scrollStyle.backgroundColor,
          scrollPaddingBottom: Number.parseFloat(scrollStyle.paddingBottom),
        };
      });

    const initialDockLayout = await inspectConversationDock();
    expect(initialDockLayout.dockParentIsCanvas).toBe(true);
    expect(initialDockLayout.dockPosition).toBe('absolute');
    expect(initialDockLayout.dockBackground).toBe('rgba(0, 0, 0, 0)');
    expect(initialDockLayout.scrollBackground).toBe(initialDockLayout.canvasBackground);
    expect(
      Math.abs(initialDockLayout.canvasBottom - initialDockLayout.dockBottom),
    ).toBeLessThanOrEqual(1);
    expect(initialDockLayout.canvasBottom - initialDockLayout.composerBottom).toBeGreaterThan(0);
    expect(initialDockLayout.scrollPaddingBottom).toBeGreaterThan(initialDockLayout.dockHeight);

    for (let index = 0; index < 20; index += 1) await composerSeparator.press('ArrowUp');
    await expect(composerSeparator).toHaveAttribute('aria-valuenow', '260');
    const expandedDockLayout = await inspectConversationDock();
    expect(expandedDockLayout.dockHeight).toBeGreaterThan(initialDockLayout.dockHeight);
    expect(expandedDockLayout.scrollPaddingBottom).toBeGreaterThan(
      initialDockLayout.scrollPaddingBottom,
    );
    expect(expandedDockLayout.scrollPaddingBottom).toBeGreaterThan(expandedDockLayout.dockHeight);

    for (let index = 0; index < 20; index += 1) await composerSeparator.press('ArrowDown');
    await expect(composerSeparator).toHaveAttribute('aria-valuenow', '54');

    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(900, 720);
    });
    await expect
      .poll(async () => {
        const layout = await inspectConversationDock();
        return layout.scrollPaddingBottom - layout.dockHeight;
      })
      .toBeGreaterThan(0);
    const narrowDockLayout = await inspectConversationDock();
    expect(narrowDockLayout.dockParentIsCanvas).toBe(true);
    expect(narrowDockLayout.scrollPaddingBottom).toBeGreaterThan(narrowDockLayout.dockHeight);
    expect(
      Math.abs(narrowDockLayout.canvasBottom - narrowDockLayout.dockBottom),
    ).toBeLessThanOrEqual(1);
    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setSize(1_600, 900);
    });

    // Read the three boxes in one frame. The layout is still settling after the resize, and
    // three separate reads each saw a different step of it.
    const { controls, leading, trailing } = await window.evaluate(() => {
      const box = (selector: string) => {
        const element = document.querySelector(selector);
        if (element === null) return null;
        const { x, width } = element.getBoundingClientRect();
        return { x, width };
      };
      return {
        controls: box('.composer-controls'),
        leading: box('.composer-controls-leading'),
        trailing: box('.composer-controls-trailing'),
      };
    });
    if (controls === null || leading === null || trailing === null) {
      throw new Error('Composer controls must be measurable');
    }
    expect(Math.abs(leading.x - controls.x)).toBeLessThanOrEqual(1);
    expect(Math.abs(trailing.x + trailing.width - controls.x - controls.width)).toBeLessThanOrEqual(
      1,
    );
    await expect(window.locator('.notification-toast')).toHaveCount(0, { timeout: 6_000 });

    const workspaceScreenshot = testInfo.outputPath('p05-workspace.png');
    await window.screenshot({ path: workspaceScreenshot });
    await testInfo.attach('P05 workspace shell', {
      contentType: 'image/png',
      path: workspaceScreenshot,
    });

    await window.getByRole('button', { name: /^文件/u }).click();
    // The 文件 tab shows the workspace's own files, read only — never demonstration data.
    // With no file open the tree has the whole page; there is no empty file view beside it.
    await expect(window.locator('.files-panel:not([hidden]) .files-layout')).toHaveAttribute(
      'data-view',
      'none',
    );
    await expect(window.getByText('打开的文件会显示在这里')).toHaveCount(0);
    await expect(window.getByRole('navigation', { name: '文件树' })).toBeVisible();
    await expect(window.getByText('目录预览 · 演示数据')).toHaveCount(0);

    const filesScreenshot = testInfo.outputPath('p05-files-workspace.png');
    await window.screenshot({ path: filesScreenshot });
    await testInfo.attach('P05 files tool workspace', {
      contentType: 'image/png',
      path: filesScreenshot,
    });

    await window.getByRole('button', { name: '设置' }).click();
    await expect(window.getByRole('heading', { level: 1, name: '设置' })).toBeVisible();
    await expect(window.getByRole('heading', { level: 2, name: '模型配置' })).toBeVisible({
      timeout: 15_000,
    });
    await expect(window.locator('.app-sidebar')).toHaveCount(0);
    await expect(window.locator('.conversation-workspace')).toHaveCount(0);
    // Catalog networking is intentionally real in this shell test. CI may be
    // online, offline, or restoring an LKG, so assert the authoritative state
    // surface rather than coupling unrelated UI coverage to one network result.
    await expect(
      window.getByText(
        /^models\.dev 目录 · (最新|缓存|正在加载|正在刷新|刷新失败，使用缓存|不可用)$/u,
      ),
    ).toBeVisible();
    await expect(window.getByText('尚无已配置模型')).toBeVisible();

    await window.getByRole('button', { name: /添加提供商.*创建自定义 Provider/u }).click();
    const providerDialog = window.getByRole('dialog', { name: '添加自定义 Provider' });
    await expect(providerDialog).toBeVisible();
    await expect(providerDialog.getByRole('textbox', { name: 'Provider 名称' })).toBeVisible();
    await expect(providerDialog.getByRole('textbox', { name: '稳定 Slug' })).toHaveCount(0);
    await expect(providerDialog.getByRole('button', { name: '保存 Provider' })).toBeDisabled();
    await expect(providerDialog.getByLabel(/API Key/u)).toHaveCount(0);

    const modelScreenshot = testInfo.outputPath('p05-model-settings.png');
    await window.screenshot({ path: modelScreenshot });
    await testInfo.attach('P05 model settings prerequisite', {
      contentType: 'image/png',
      path: modelScreenshot,
    });

    expect(pageErrors).toEqual([]);
    expect(consoleErrors).toEqual([]);
  } finally {
    await closeAura(application);
  }
});

// eslint-disable-next-line no-empty-pattern -- Playwright requires object destructuring even when an Electron-only test uses no browser fixtures.
test('P04 security control plane uses the light theme and keeps Host Preparation probe internal', async ({}, testInfo) => {
  const workspaceRoot = await mkdtemp(join(tmpdir(), 'aura-p04-ui-'));
  const application = await launchAura();
  try {
    const window = await application.firstWindow();
    const pageErrors: string[] = [];
    window.on('pageerror', (error) => pageErrors.push(error.message));

    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const created = await window.evaluate(async (selectedRoot) => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      return rendererGlobal.aura.v1.createWorkspace({
        display_name: 'P04 UI Fixture',
        selected_root: selectedRoot,
      });
    }, workspaceRoot);
    expect(created.ok).toBe(true);
    await window.getByRole('button', { name: '设置' }).click();
    await window.getByRole('button', { name: '权限与沙箱' }).click();
    await expect(window.getByRole('heading', { level: 2, name: '权限与沙箱' })).toBeVisible();
    await expect(window.getByRole('heading', { name: '会话默认权限' })).toBeVisible();
    await expect(window.getByRole('heading', { name: '待处理审批' })).toHaveCount(0);
    // No explanatory line under the panel.
    await expect(window.getByText(/兼容环境用于运行隔离工具/u)).toHaveCount(0);

    const palette = await window.evaluate(() => {
      const rendererDocument = Reflect.get(globalThis, 'document') as {
        documentElement: unknown;
        querySelector(selector: string): unknown | null;
      };
      const computedStyle = Reflect.get(globalThis, 'getComputedStyle') as (element: unknown) => {
        backgroundColor: string;
        colorScheme: string;
      };
      const shell = rendererDocument.querySelector('.app-shell');
      const securityCard = rendererDocument.querySelector('.settings-workspace');
      const securityPanel = rendererDocument.querySelector('.settings-surface');
      if (shell === null || securityCard === null || securityPanel === null) {
        throw new Error('P04 security surface is incomplete');
      }
      return {
        colorScheme: computedStyle(rendererDocument.documentElement).colorScheme,
        panelBackground: computedStyle(securityPanel).backgroundColor,
        securityBackground: computedStyle(securityCard).backgroundColor,
        shellBackground: computedStyle(shell).backgroundColor,
      };
    });
    expect(palette).toEqual({
      colorScheme: 'light',
      panelBackground: 'rgb(255, 255, 255)',
      securityBackground: 'rgb(255, 255, 255)',
      shellBackground: 'rgb(251, 251, 250)',
    });

    await expect(window.getByRole('button', { name: '重新探测' })).toHaveCount(0);
    await expect(window.locator('.security-error')).toHaveCount(0);
    expect(pageErrors).toEqual([]);

    const screenshotPath = testInfo.outputPath('p04-security-light.png');
    await window.screenshot({ fullPage: true, path: screenshotPath });
    await testInfo.attach('P04 light security control plane', {
      contentType: 'image/png',
      path: screenshotPath,
    });
  } finally {
    await closeAura(application);
    await rm(workspaceRoot, { force: true, recursive: true });
  }
});

test('Archive completes through the Main-owned Lifecycle Barrier', async () => {
  const workspaceRoot = await mkdtemp(join(tmpdir(), 'aura-workspace-e2e-'));
  const application = await launchAura();
  try {
    const window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const result = await window.evaluate(async (selectedRoot) => {
      const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
      const created = await rendererGlobal.aura.v1.createWorkspace({
        display_name: 'Lifecycle E2E',
        selected_root: selectedRoot,
      });
      if (!created.ok) return { archive: undefined, created, state: undefined };
      const archive = await rendererGlobal.aura.v1.archiveWorkspace({
        expected_workspace_revision: created.value.workspace.revision,
        workspace_id: created.value.workspace.workspace_id,
      });
      // The window itself answers the barrier's preparation (its editor keeps no unsaved
      // buffers), so the archive completes without anyone else acknowledging it.
      let state: string | undefined;
      for (let attempt = 0; attempt < 100 && state !== 'archived'; attempt += 1) {
        const projection = await rendererGlobal.aura.v1.getWorkspaceSessions();
        state = projection.ok
          ? projection.value.payload.workspaces.find(
              (workspace) => workspace.workspace_id === created.value.workspace.workspace_id,
            )?.state
          : undefined;
        if (state !== 'archived') await new Promise((resolve) => setTimeout(resolve, 100));
      }
      return { archive, created, state };
    }, workspaceRoot);
    expect(result.created.ok).toBe(true);
    expect(result.archive?.ok).toBe(true);
    if (result.archive?.ok) expect(result.archive.value.state).toBe('preparing');
    expect(result.state).toBe('archived');
  } finally {
    await closeAura(application);
    await rm(workspaceRoot, { force: true, recursive: true });
  }
});
