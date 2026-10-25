import { expect, test } from '@playwright/test';
import {
  RendererAura,
  launchAura,
  restartAura,
  closeAura,
  createShellSession,
} from './electron-shell/harness.js';

// eslint-disable-next-line no-empty-pattern -- Playwright requires object destructuring even when an Electron-only test uses no browser fixtures.
test('workspace sidebars resize, collapse, recover preferences, and never overlap', async ({}, testInfo) => {
  test.setTimeout(60_000);
  let application = await launchAura();
  try {
    let window = await application.firstWindow();
    await expect(window.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 15_000 });
    const session = await createShellSession(application, window);
    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setContentSize(1_400, 800);
    });
    await expect
      .poll(() => window.evaluate(() => Number(Reflect.get(globalThis, 'innerWidth'))))
      .toBeGreaterThanOrEqual(1_390);
    await expect
      .poll(async () =>
        Number(
          await window.locator('.workspace-main').getAttribute('data-responsive-candidate-width'),
        ),
      )
      .toBeGreaterThanOrEqual(544);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();

    const paneBounds = () =>
      window.evaluate(() => {
        interface MeasurableElement {
          getBoundingClientRect(): { height: number; width: number; x: number; y: number };
        }
        const selectors = {
          conversation: '.conversation-workspace',
          left: '.app-sidebar',
          leftSeparator: '.left-panel-resizer',
          right: '.right-workspace',
          rightSeparator: '.right-panel-resizer',
        } as const;
        const rendererDocument = Reflect.get(globalThis, 'document') as {
          querySelector(selector: string): MeasurableElement | null;
        };
        const readRect = (selector: string) => {
          const element = rendererDocument.querySelector(selector);
          if (element === null) {
            throw new Error(`Workspace pane is not measurable: ${selector}`);
          }
          const rect = element.getBoundingClientRect();
          return { height: rect.height, width: rect.width, x: rect.x, y: rect.y };
        };
        return {
          conversation: readRect(selectors.conversation),
          left: readRect(selectors.left),
          leftSeparator: readRect(selectors.leftSeparator),
          right: readRect(selectors.right),
          rightSeparator: readRect(selectors.rightSeparator),
        };
      });
    const expectAdjacentPanes = async () => {
      const bounds = await paneBounds();
      expect(bounds.left.x + bounds.left.width).toBeLessThanOrEqual(bounds.leftSeparator.x + 1);
      expect(bounds.leftSeparator.x + bounds.leftSeparator.width).toBeLessThanOrEqual(
        bounds.conversation.x + 1,
      );
      expect(bounds.conversation.x + bounds.conversation.width).toBeLessThanOrEqual(
        bounds.rightSeparator.x + 1,
      );
      expect(bounds.rightSeparator.x + bounds.rightSeparator.width).toBeLessThanOrEqual(
        bounds.right.x + 1,
      );
    };
    await expectAdjacentPanes();

    const leftSeparator = window.getByRole('separator', { name: '调整左侧工作区宽度' });
    const rightSeparator = window.getByRole('separator', { name: '调整右侧工作区宽度' });
    const initialLeftWidth = Number(await leftSeparator.getAttribute('aria-valuenow'));
    const initialRightWidth = Number(await rightSeparator.getAttribute('aria-valuenow'));
    const maximumLeftWidth = Number(await leftSeparator.getAttribute('aria-valuemax'));
    const maximumRightWidth = Number(await rightSeparator.getAttribute('aria-valuemax'));
    await leftSeparator.press('ArrowRight');
    await rightSeparator.press('ArrowLeft');
    await expect(leftSeparator).toHaveAttribute(
      'aria-valuenow',
      String(Math.min(maximumLeftWidth, initialLeftWidth + 16)),
    );
    await expect(rightSeparator).toHaveAttribute(
      'aria-valuenow',
      String(Math.min(maximumRightWidth, initialRightWidth + 16)),
    );

    const setCandidateConversationWidth = async (candidateWidth: number) => {
      for (let attempt = 0; attempt < 5; attempt += 1) {
        const metrics = await window.evaluate(() => {
          const rendererDocument = Reflect.get(globalThis, 'document') as {
            querySelector(selector: string): { getAttribute(name: string): string | null } | null;
          };
          return {
            candidate: Number(
              rendererDocument
                .querySelector('.workspace-main')
                ?.getAttribute('data-responsive-candidate-width'),
            ),
            viewportWidth: Number(Reflect.get(globalThis, 'innerWidth')),
          };
        });
        const delta = candidateWidth - metrics.candidate;
        const targetReached =
          candidateWidth < 480
            ? metrics.candidate <= candidateWidth
            : metrics.candidate >= candidateWidth;
        if (targetReached) return;
        const adjustment = Math.sign(delta) * Math.max(8, Math.abs(Math.round(delta)));
        await application.evaluate(
          ({ BrowserWindow }, width) => {
            BrowserWindow.getAllWindows()[0]?.setContentSize(width, 720);
          },
          Math.max(800, Math.round(metrics.viewportWidth + adjustment)),
        );
        await expect
          .poll(async () =>
            Number(
              await window
                .locator('.workspace-main')
                .getAttribute('data-responsive-candidate-width'),
            ),
          )
          .not.toBe(metrics.candidate);
      }
    };

    await setCandidateConversationWidth(479);
    await expect
      .poll(async () =>
        Number(
          await window.locator('.workspace-main').getAttribute('data-responsive-candidate-width'),
        ),
      )
      .toBeLessThan(480);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);
    await expect(window.getByRole('complementary', { name: '工作区导航' })).toBeVisible();
    await setCandidateConversationWidth(544);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();

    await setCandidateConversationWidth(479);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);
    await window.getByRole('button', { name: '展开右侧边栏' }).click();
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();
    await expectAdjacentPanes();
    const narrowConversation = await window.locator('.conversation-workspace').boundingBox();
    const narrowDock = await window.locator('.conversation-dock').boundingBox();
    if (narrowConversation === null || narrowDock === null) {
      throw new Error('Narrow conversation and dock must be measurable');
    }
    expect(narrowDock.x).toBeGreaterThanOrEqual(narrowConversation.x);
    expect(narrowDock.x + narrowDock.width).toBeLessThanOrEqual(
      narrowConversation.x + narrowConversation.width,
    );
    await expect(window.locator('.todo-dock')).toHaveCount(0);

    await setCandidateConversationWidth(544);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();
    await setCandidateConversationWidth(479);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);

    await setCandidateConversationWidth(544);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();
    await window.getByRole('button', { name: '关闭右侧边栏' }).click();
    await setCandidateConversationWidth(700);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);

    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setContentSize(800, 720);
    });
    await expect(window.getByRole('complementary', { name: '工作区导航' })).toBeVisible();
    await window.getByRole('button', { name: '展开右侧边栏' }).click();
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();
    await expectAdjacentPanes();
    const squeezedConversation = await window.locator('.conversation-workspace').boundingBox();
    const squeezedDock = await window.locator('.conversation-dock').boundingBox();
    if (squeezedConversation === null || squeezedDock === null) {
      throw new Error('Squeezed conversation and dock must be measurable');
    }
    expect(squeezedDock.x).toBeGreaterThanOrEqual(squeezedConversation.x);
    expect(squeezedDock.x + squeezedDock.width).toBeLessThanOrEqual(
      squeezedConversation.x + squeezedConversation.width,
    );
    const narrowScreenshot = testInfo.outputPath('workspace-three-pane-squeezed.png');
    await window.screenshot({ path: narrowScreenshot });
    await testInfo.attach('Three adjacent panes at the 800px minimum window', {
      contentType: 'image/png',
      path: narrowScreenshot,
    });

    await application.evaluate(({ BrowserWindow }) => {
      BrowserWindow.getAllWindows()[0]?.setContentSize(1_400, 800);
    });
    const wideLeftSeparator = window.getByRole('separator', { name: '调整左侧工作区宽度' });
    const leftWidth = Number(await wideLeftSeparator.getAttribute('aria-valuenow'));
    const leftMinimum = Number(await wideLeftSeparator.getAttribute('aria-valuemin'));
    const leftBox = await wideLeftSeparator.boundingBox();
    if (leftBox === null) throw new Error('Left separator must be measurable');
    const leftCenterX = leftBox.x + leftBox.width / 2;
    const leftCenterY = leftBox.y + leftBox.height / 2;
    const leftCollapseX = leftCenterX - (leftWidth - leftMinimum + 50);
    const leftReopenX = leftCenterX - (leftWidth - leftMinimum);
    await window.mouse.move(leftBox.x + leftBox.width / 2, leftBox.y + leftBox.height / 2);
    await window.mouse.down();
    await window.mouse.move(leftCollapseX, leftCenterY, { steps: 6 });
    await expect(window.locator('.left-panel-resizer')).toHaveAttribute(
      'data-collapse-armed',
      'true',
    );
    await expect(window.getByRole('complementary', { name: '工作区导航' })).toHaveCount(0);
    await window.mouse.move(leftReopenX, leftCenterY, { steps: 6 });
    await expect(window.locator('.left-panel-resizer')).toHaveAttribute(
      'data-collapse-armed',
      'false',
    );
    await expect(window.getByRole('complementary', { name: '工作区导航' })).toBeVisible();
    await window.mouse.move(leftCollapseX, leftCenterY, { steps: 6 });
    await window.mouse.up();
    await expect(window.getByRole('complementary', { name: '工作区导航' })).toHaveCount(0);

    const wideRightSeparator = window.getByRole('separator', { name: '调整右侧工作区宽度' });
    const rightWidth = Number(await wideRightSeparator.getAttribute('aria-valuenow'));
    const rightMinimum = Number(await wideRightSeparator.getAttribute('aria-valuemin'));
    const rightBox = await wideRightSeparator.boundingBox();
    if (rightBox === null) throw new Error('Right separator must be measurable');
    const rightCenterX = rightBox.x + rightBox.width / 2;
    const rightCenterY = rightBox.y + rightBox.height / 2;
    const rightCollapseX = rightCenterX + (rightWidth - rightMinimum + 50);
    const rightReopenX = rightCenterX + (rightWidth - rightMinimum);
    await window.mouse.move(rightCenterX, rightCenterY);
    await window.mouse.down();
    await window.mouse.move(rightCollapseX, rightCenterY, { steps: 6 });
    await expect(window.locator('.right-panel-resizer')).toHaveAttribute(
      'data-collapse-armed',
      'true',
    );
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);
    await window.mouse.move(rightReopenX, rightCenterY, { steps: 6 });
    await expect(window.locator('.right-panel-resizer')).toHaveAttribute(
      'data-collapse-armed',
      'false',
    );
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toBeVisible();
    await window.mouse.move(rightCollapseX, rightCenterY, { steps: 6 });
    await window.mouse.up();
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);

    const shellBox = await window.locator('.workspace-shell').boundingBox();
    const leftReopen = window.getByRole('button', { name: '展开左侧边栏' });
    const rightReopen = window.getByRole('button', { name: '展开右侧边栏' });
    const leftReopenBox = await leftReopen.boundingBox();
    const rightReopenBox = await rightReopen.boundingBox();
    if (shellBox === null || leftReopenBox === null || rightReopenBox === null) {
      throw new Error('Sidebar reopen controls must be measurable');
    }
    expect(leftReopenBox.x - shellBox.x).toBeLessThanOrEqual(12);
    expect(
      shellBox.x + shellBox.width - (rightReopenBox.x + rightReopenBox.width),
    ).toBeLessThanOrEqual(12);

    application = await restartAura(application);
    window = await application.firstWindow();
    await expect
      .poll(
        () =>
          window.evaluate(async () => {
            const rendererGlobal = globalThis as typeof globalThis & { aura: RendererAura };
            const result = await rendererGlobal.aura.v1.getCoreHealth();
            return result.ok ? result.value.payload.state : 'projection_error';
          }),
        { timeout: 15_000 },
      )
      .toBe('ready');
    await expect(window.getByRole('heading', { level: 2, name: session.title })).toBeVisible();
    await expect(window.getByRole('complementary', { name: '工作区导航' })).toHaveCount(0);
    await expect(window.getByRole('complementary', { name: '右侧工作区' })).toHaveCount(0);
    await window.getByRole('button', { name: '展开左侧边栏' }).click();
    await window.getByRole('button', { name: '展开右侧边栏' }).click();
    await expect(window.getByRole('separator', { name: '调整左侧工作区宽度' })).toHaveAttribute(
      'aria-valuenow',
      String(Math.min(maximumLeftWidth, initialLeftWidth + 16)),
    );
    await expect(window.getByRole('separator', { name: '调整右侧工作区宽度' })).toHaveAttribute(
      'aria-valuenow',
      String(Math.min(maximumRightWidth, initialRightWidth + 16)),
    );
  } finally {
    await closeAura(application);
  }
});
