/* global MutationObserver, document, getComputedStyle, window */
// Focused B04 measurement: open the grant dialog from the production entry,
// click its close control, and report the native dialog state before/after.
import { _electron as electron } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const desktop = resolve(import.meta.dirname, '..');
const output = resolve(desktop, '../../target/r10-grant-close');
const localAppData = join(output, 'Local');
const roaming = join(output, 'Roaming');
const workspaceRoot = join(output, 'synthetic-workspace');
for (const dir of [localAppData, roaming, join(output, 'Electron'), workspaceRoot])
  await mkdir(dir, { recursive: true });

const state = async (page) =>
  page.evaluate(() => ({
    nodes: [...document.querySelectorAll('dialog.modal-backdrop')].map((dialog) => ({
      open: dialog.open,
      modal: dialog.matches(':modal'),
      display: getComputedStyle(dialog).display,
      label:
        dialog.querySelector('h2')?.textContent ??
        dialog.querySelector('[role="dialog"]')?.getAttribute('aria-labelledby') ??
        'unlabelled',
    })),
    // Live React fibers that own a <dialog>, versus the DOM nodes above: a
    // larger node count means portalled layers outlive their components.
    liveDialogs: (() => {
      const container = document.querySelector('#root');
      if (!container) return -1;
      const key = Object.keys(container).find((name) => name.startsWith('__reactContainer$'));
      const host = key ? container[key] : undefined;
      let live = 0;
      const counts = {};
      const visit = (fiber) => {
        let node = fiber;
        while (node) {
          counts.total = (counts.total ?? 0) + 1;
          const element = node.stateNode;
          if (element && element.tagName === 'DIALOG') live += 1;
          if (
            element &&
            element.className &&
            String(element.className).includes('conversation-workspace')
          )
            counts.sections = (counts.sections ?? 0) + 1;
          if (node.child) visit(node.child);
          node = node.sibling;
        }
      };
      if (host?.current) visit(host.current);
      else if (host) visit(host);
      window.__r10Counts = counts;
      return live;
    })(),
  }));

let app;
const report = { steps: [] };
try {
  app = await electron.launch({
    args: [`--user-data-dir=${join(output, 'Electron')}`, desktop],
    cwd: desktop,
    env: {
      ...process.env,
      APPDATA: roaming,
      LOCALAPPDATA: localAppData,
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
    timeout: 60_000,
  });
  const page = await app.firstWindow();
  const consoleLines = [];
  page.on('console', (message) => {
    if (message.text().includes('[r10-debug]')) consoleLines.push(message.text());
  });
  page.on('pageerror', (error) => consoleLines.push(`pageerror: ${error.message.slice(0, 200)}`));
  await page.waitForLoadState('domcontentloaded');
  for (let attempt = 0; attempt < 90; attempt += 1) {
    const ready = await page.evaluate(async () => {
      const result = await window.aura.v1.getCoreHealth();
      return result.ok && result.value.payload.state === 'ready';
    });
    if (ready) break;
    await page.waitForTimeout(1000);
  }
  const workspace = await page.evaluate(async (root) => {
    const result = await window.aura.v1.createWorkspace({
      display_name: 'Close fixture',
      selected_root: root,
    });
    return result.ok ? result.value.workspace.workspace_id : undefined;
  }, workspaceRoot);
  await page.evaluate(
    async (workspaceId) => window.aura.v1.createSession({ workspace_id: workspaceId, title: '' }),
    workspace,
  );
  await page.waitForTimeout(800);

  // Watch the document for portal layer insertions and removals.
  await page.evaluate(() => {
    window.__r10 = { added: 0, removed: 0 };
    new MutationObserver((records) => {
      for (const record of records) {
        for (const node of record.addedNodes)
          if (node.tagName === 'DIALOG') window.__r10.added += 1;
        for (const node of record.removedNodes)
          if (node.tagName === 'DIALOG') window.__r10.removed += 1;
      }
    }).observe(document.body, { childList: true });
  });

  // Idle growth: does the conversation view remount on its own?
  const sample = async () => {
    const value = await state(page);
    return {
      nodes: value.nodes.length,
      open: value.nodes.filter((entry) => entry.open).length,
      live: value.liveDialogs,
    };
  };
  report.idle_t1 = await sample();
  // Is the conversation view itself being replaced (which would remount every
  // dialog it owns)? Sample the layout at 250 ms resolution.
  report.flicker = await page.evaluate(async () => {
    const series = [];
    for (let tick = 0; tick < 24; tick += 1) {
      series.push(
        [
          document.querySelectorAll('.conversation-workspace').length,
          document.querySelectorAll('section.composer').length,
          document.querySelectorAll('dialog.modal-backdrop').length,
          document.querySelector('.new-session-workspace') === null ? 'w' : 'd',
        ].join('/'),
      );
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
    return [...new Set(series)];
  });
  await page.waitForTimeout(5_000);
  report.idle_t6 = await sample();
  await page.waitForTimeout(5_000);
  report.idle_t11 = await sample();

  report.before_open = await sample();
  await page.locator('.composer-select').first().click();
  await page.getByRole('menuitem', { name: '会话授权管理', exact: true }).first().click();
  await page.waitForTimeout(500);
  report.after_open = await sample();
  report.open_heading_visible = await page
    .getByRole('heading', { name: '会话授权管理', exact: true })
    .first()
    .isVisible();

  await page
    .getByRole('button', { name: '关闭授权管理', exact: true })
    .first()
    .click({ force: true });
  await page.waitForTimeout(800);
  report.after_close_click = await sample();
  report.heading_visible_after_close = await page
    .getByRole('heading', { name: '会话授权管理', exact: true })
    .first()
    .isVisible()
    .catch(() => false);
  // Can the composer be used again (no invisible modal layer left behind)?
  report.composer_clickable = await page
    .locator('.composer-select')
    .first()
    .click({ timeout: 5_000 })
    .then(() => true)
    .catch(() => false);
  report.mutations = await page.evaluate(() => window.__r10);
  report.open_labels = await page.evaluate(() =>
    [...document.querySelectorAll('dialog.modal-backdrop[open]')].map((dialog) => ({
      label:
        dialog.querySelector('h2')?.textContent ??
        dialog.querySelector('[role="dialog"]')?.getAttribute('aria-labelledby') ??
        'unlabelled',
      text: (dialog.textContent ?? '').trim().slice(0, 40),
    })),
  );
  report.debug = consoleLines.slice(-14);
  report.openLines = consoleLines.filter((line) => line.includes('[r10-debug] open'));
  report.roots = await page.evaluate(() => {
    const describe = (element) => {
      const key = Object.keys(element).find((name) => name.startsWith('__reactFiber$'));
      let fiber = key ? element[key] : undefined;
      let depth = 0;
      while (fiber?.return) {
        fiber = fiber.return;
        depth += 1;
      }
      const container = fiber?.stateNode?.containerInfo;
      return {
        open: element.open,
        depth,
        container:
          container === undefined
            ? 'none'
            : `${container.tagName?.toLowerCase() ?? '?'}#${container.id ?? ''}.${String(container.className ?? '').slice(0, 24)} connected=${container.isConnected}`,
        isMainRoot: container === document.querySelector('#root'),
      };
    };
    return [...document.querySelectorAll('dialog.modal-backdrop')].map(describe);
  });
  report.mountLines = consoleLines.filter((line) => line.includes('modal mount')).slice(0, 14);
  report.tally = consoleLines.reduce((accumulator, line) => {
    const key = line.includes('modal mount')
      ? 'mounts'
      : line.includes('modal unmount')
        ? 'unmounts'
        : line.includes('setDialogOpen true')
          ? 'opens'
          : line.includes('setDialogOpen false')
            ? 'closes'
            : line.includes('grants onClose')
              ? 'onClose'
              : line.includes('pageerror')
                ? 'pageerrors'
                : 'other';
    accumulator[key] = (accumulator[key] ?? 0) + 1;
    return accumulator;
  }, {});
  report.pageerrors = consoleLines.filter((line) => line.includes('pageerror')).slice(0, 5);
  report.entries = await page.evaluate(() => window.__r10Entries ?? null);
  report.fiberCounts = await page.evaluate(() => window.__r10Counts ?? null);
  report.bodySummary = await page.evaluate(() =>
    [...document.body.children].map(
      (element) =>
        `${element.tagName.toLowerCase()}.${String(element.className).slice(0, 30)}(${element.childElementCount})`,
    ),
  );
  await page.keyboard.press('Escape');
  await page.screenshot({ path: join(output, 'after-close.png') });
  console.log(JSON.stringify(report));
} catch (error) {
  report.error = error.message.slice(0, 600);
  console.log(JSON.stringify(report));
  process.exitCode = 1;
} finally {
  await writeFile(join(output, 'report.json'), JSON.stringify(report, null, 2));
  await app?.close();
}
