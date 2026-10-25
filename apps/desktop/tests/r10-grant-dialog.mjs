/* global document, getComputedStyle, window */
// Real-window reproduction for B04: "会话授权管理" must close from the
// production entry (composer permission menu) via X, Escape and the backdrop.
//
// This is not a Playwright spec: it launches the built desktop app with an
// isolated data root, drives the real UI, and writes evidence to
// target/r10-grant-dialog/. It touches no user data and closes the app.
import { _electron as electron } from '@playwright/test';
import { mkdir, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';

const desktop = resolve(import.meta.dirname, '..');
const output = resolve(desktop, '../../target/r10-grant-dialog');
const localAppData = join(output, 'Local');
const roaming = join(output, 'Roaming');
const workspaceRoot = join(output, 'synthetic-workspace');
for (const dir of [localAppData, roaming, join(output, 'Electron'), workspaceRoot])
  await mkdir(dir, { recursive: true });

const steps = [];
const errors = [];
const record = (name, detail) => {
  steps.push({ name, detail });
  console.log(
    `${steps.length}. ${name}${detail === undefined ? '' : ` -> ${JSON.stringify(detail)}`}`,
  );
};

let app;
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
  record('windows', app.windows().length);
  page.on('pageerror', (error) => errors.push(`pageerror: ${error.message}`));
  page.on('console', (message) => {
    if (message.type() === 'error') errors.push(`console: ${message.text()}`);
  });
  await page.waitForLoadState('domcontentloaded');
  for (let attempt = 0; attempt < 90; attempt += 1) {
    const ready = await page.evaluate(async () => {
      const result = await window.aura.v1.getCoreHealth();
      return result.ok && result.value.payload.state === 'ready';
    });
    if (ready) break;
    await page.waitForTimeout(1000);
  }
  record('core ready', page.url());

  const workspace = await page.evaluate(async (root) => {
    const result = await window.aura.v1.createWorkspace({
      display_name: 'Grant dialog fixture',
      selected_root: root,
    });
    return result.ok ? { ok: true, id: result.value.workspace.workspace_id } : result;
  }, workspaceRoot);
  record('workspace created', workspace);
  if (!workspace.ok) throw new Error(`workspace create failed: ${JSON.stringify(workspace)}`);

  const session = await page.evaluate(async (workspaceId) => {
    const result = await window.aura.v1.createSession({ workspace_id: workspaceId, title: '' });
    return result.ok ? { ok: true, id: result.value.session_id } : result;
  }, workspace.id);
  record('unnamed session created', session);
  if (!session.ok) throw new Error(`session create failed: ${JSON.stringify(session)}`);

  // Open the session through the real sidebar entry.
  await page.reload();
  const sessionEntry = page
    .getByRole('button', { name: /新会话|Grant dialog fixture|^1$/u })
    .first();
  await sessionEntry.click().catch(() => undefined);
  await page.waitForTimeout(500);
  const inConversation = await page.locator('.composer-select').count();
  record('conversation composer present', inConversation);

  const openGrants = async () => {
    await page.locator('.composer-select').first().click();
    const menuItems = await page
      .getByRole('menuitem', { name: '会话授权管理', exact: true })
      .count();
    const dialogsBefore = await page.locator('dialog.modal-backdrop').count();
    await page.getByRole('menuitem', { name: '会话授权管理', exact: true }).first().click();
    const dialogsAfterClick = await page.locator('dialog.modal-backdrop').count();
    await page.waitForTimeout(300);
    record('menu item / dialogs', {
      menuItems,
      dialogsBefore,
      dialogsAfterClick,
      dialogsSettled: await page.locator('dialog.modal-backdrop').count(),
    });
    await page
      .getByRole('heading', { name: '会话授权管理', exact: true })
      .first()
      .waitFor({ timeout: 10_000 });
  };
  // Duplicate mounts are the reported failure mode: report every instance and
  // where it lives in the real ancestor layer.
  const structure = async () =>
    page.evaluate(() => {
      const ownerStack = (element) => {
        const key = Object.keys(element).find((name) => name.startsWith('__reactFiber$'));
        const stack = [];
        let fiber = key ? element[key] : undefined;
        for (let step = 0; fiber && step < 60; step += 1) {
          const type = fiber.type;
          const name =
            typeof type === 'string'
              ? null
              : typeof type === 'function'
                ? (type.displayName ?? type.name ?? '')
                : '';
          if (name) stack.push(name);
          fiber = fiber.return;
        }
        return stack;
      };
      const describe = (element) => {
        const path = [];
        for (let node = element; node && path.length < 8; node = node.parentElement)
          path.push(
            `${node.tagName.toLowerCase()}${node.className ? `.${String(node.className).split(' ').slice(0, 2).join('.')}` : ''}`,
          );
        const rect = element.getBoundingClientRect();
        return {
          path: path.join(' < '),
          open: element.open ?? null,
          modal: element.matches(':modal'),
          inert: element.closest('[inert]') !== null,
          ariaHidden: element.closest('[aria-hidden="true"]') !== null,
          display: getComputedStyle(element).display,
          rect: [
            Math.round(rect.x),
            Math.round(rect.y),
            Math.round(rect.width),
            Math.round(rect.height),
          ],
          ownerStack: ownerStack(element),
        };
      };
      return {
        dialogs: [...document.querySelectorAll('dialog.modal-backdrop')].map(describe),
        headings: [...document.querySelectorAll('#session-grants-title')].map(describe),
        composers: document.querySelectorAll('.composer-select').length,
        conversations: document.querySelectorAll('.conversation-workspace').length,
        editors: document.querySelectorAll('[aria-label="消息编辑器"]').length,
        tabs: document.querySelectorAll('dialog.modal-backdrop button').length,
        reactRoots: [...document.querySelectorAll('*')]
          .filter((element) =>
            Object.keys(element).some((name) => name.startsWith('__reactContainer$')),
          )
          .map(
            (element) =>
              `${element.tagName.toLowerCase()}#${element.id}.${String(element.className).slice(0, 40)}`,
          ),
        bodyChildren: [...document.body.children].map(
          (element) => `${element.tagName.toLowerCase()}.${String(element.className).slice(0, 40)}`,
        ),
      };
    });
  const dialogState = async () =>
    page.evaluate(() => {
      const dialogs = [...document.querySelectorAll('dialog.modal-backdrop')];
      if (dialogs.length === 0) return { present: false };
      return {
        present: true,
        count: dialogs.length,
        first: {
          open: dialogs[0].open,
          modal: dialogs[0].matches(':modal'),
          pointerEvents: getComputedStyle(dialogs[0]).pointerEvents,
        },
      };
    });
  const headingVisible = () =>
    page
      .getByRole('heading', { name: '会话授权管理', exact: true })
      .first()
      .isVisible()
      .catch(() => false);

  await openGrants();
  record('dialog opened', await dialogState());
  record('structure after open', await structure());
  await page.screenshot({ path: join(output, 'opened.png') });

  const attempts = [];
  // 1. The dedicated close control.
  await page
    .getByRole('button', { name: '关闭授权管理', exact: true })
    .first()
    .click({ timeout: 5_000 })
    .catch((error) => errors.push(`x-click: ${error.message.split('\n')[0]}`));
  await page.waitForTimeout(400);
  attempts.push({
    how: 'close-button',
    closed: !(await headingVisible()),
    state: await dialogState(),
    structure: await structure(),
  });

  // 2. Escape.
  if (!attempts.at(-1).closed) await openGrants();
  await page.keyboard.press('Escape');
  await page.waitForTimeout(400);
  attempts.push({ how: 'escape', closed: !(await headingVisible()), state: await dialogState() });

  // 3. Backdrop click outside the panel.
  if (!attempts.at(-1).closed) await openGrants();
  await page.mouse.click(12, 12);
  await page.waitForTimeout(400);
  attempts.push({ how: 'backdrop', closed: !(await headingVisible()), state: await dialogState() });
  await page.screenshot({ path: join(output, 'after-close-attempts.png') });

  // 4. After closing, the conversation must be usable and reopening must work.
  let reopened = false;
  if (attempts.every((entry) => entry.closed)) {
    await page.locator('.composer-select').first().click();
    await page.keyboard.press('Escape');
    await openGrants();
    reopened = await headingVisible();
    await page.getByRole('button', { name: '关闭授权管理', exact: true }).click();
  }
  record('close attempts', attempts);
  record('reopen works', reopened);

  await writeFile(
    join(output, 'evidence.json'),
    JSON.stringify({ steps, attempts, reopened, dialog: await dialogState(), errors }, null, 2),
  );
  console.log(
    JSON.stringify({ closed_every_way: attempts.every((entry) => entry.closed), attempts, errors }),
  );
} catch (error) {
  await writeFile(
    join(output, 'failure.json'),
    JSON.stringify({ steps, errors, message: error.message.slice(0, 3000) }, null, 2),
  );
  console.error(`FAILED: ${error.message.slice(0, 400)}`);
  process.exitCode = 1;
} finally {
  await app?.close();
}
