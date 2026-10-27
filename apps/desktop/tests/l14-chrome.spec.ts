import { expect, test, type Page, type Worker } from '@playwright/test';
import { access, mkdir, mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, relative, resolve } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import { finished, history, launch } from './s06/harness';
import {
  TEST_EXTENSION_ID,
  buildTestExtension,
  credentialPrefix,
  credentials,
  deleteCredential,
  developmentNamespace,
  hostName,
  launchChrome,
  registeredManifest,
  removeRegistration,
  runningRelays,
  startSites,
} from './l14/chrome';
import { chromeScript } from './l14/model';

// Real Electron and Core, the relay as Chrome starts it, and the test build of the extension in
// Chrome for Testing (Playwright's Chromium). Only the model is scripted. Chrome control writes
// this development namespace's host registration under HKCU and its pairing secret to the
// Credential Manager; both are removed by the test.
const TYPED = 'Aura 你好';
const relay = resolve(import.meta.dirname, '../../../target/debug/aura-chrome-host.exe');

type Envelope<T> = { ok: true; value: T } | { ok: false; error: unknown };
const value = <T>(result: Envelope<T>): T => {
  if (!result.ok) throw new Error(JSON.stringify(result.error));
  return result.value;
};
const status = async (page: Page) =>
  value(await page.evaluate(() => window.aura.v1.chrome!.status()));
const setEnabled = async (page: Page, enabled: boolean) => {
  const revision = (await status(page)).revision ?? 0;
  return value(
    await page.evaluate(async (request) => window.aura.v1.chrome!.setEnabled(request), {
      enabled,
      expected_revision: revision,
    }),
  );
};
const shares = async (page: Page, session_id: string) =>
  value(
    await page.evaluate(
      async (id) => window.aura.v1.chrome!.shares({ session_id: id }),
      session_id,
    ),
  );

/** The next approval of the session that is not among `seen`, answered with `decision`. */
async function answerNext(
  page: Page,
  session: string,
  seen: Set<string>,
  decision: 'allow_once' | 'allow_for_current_turn',
) {
  let id: string | undefined;
  await expect
    .poll(
      async () => {
        const view = await history(page, session);
        id = view.active?.pending_approval_ids.find((pending) => !seen.has(pending));
        return id !== undefined;
      },
      { timeout: 90_000 },
    )
    .toBe(true);
  seen.add(id!);
  const view = value(
    await page.evaluate(async (request) => window.aura.v1.getAgentApproval(request), {
      session_id: session,
      approval_id: id!,
    }),
  );
  const resolved = await page.evaluate(
    async (request) => window.aura.v1.resolveAgentApproval(request),
    {
      session_id: session,
      approval_id: view.approval.approval_id,
      expected_revision: view.approval.revision,
      decision,
    },
  );
  expect(resolved.ok, JSON.stringify(resolved)).toBe(true);
  return {
    capabilities: view.requested_capabilities ?? [],
    decisions: view.approval.allowed_decisions,
  };
}

/** A registration a crashed run left behind names a manifest that is gone; a live one is not ours. */
async function clearStaleRegistration(host: string) {
  const manifest = await registeredManifest(host);
  if (manifest === null) return;
  const live = await access(manifest).then(
    () => true,
    () => false,
  );
  if (live)
    throw new Error(`${host} is registered for ${manifest}; turn Chrome control off there first`);
  await removeRegistration(host);
}

async function cleanup(root: string) {
  const rel = relative(resolve(tmpdir()), resolve(root));
  if (isAbsolute(rel) || rel.startsWith('..') || !rel.startsWith('aura-l14-chrome-'))
    throw new Error(`Refusing to remove a non-fixture directory: ${root}`);
  await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}

// eslint-disable-next-line no-empty-pattern -- real processes on isolated data; see above.
test('L14 the extension pairs through the relay in Chrome for Testing and a model acts in its shared tab', async ({}, info) => {
  test.setTimeout(480_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l14-chrome-'));
  const namespace = await developmentNamespace();
  const host = hostName(namespace);
  const pairingPrefix = `${credentialPrefix(namespace)}chrome/pairing/`;
  await clearStaleRegistration(host);
  const sites = await startSites();
  const extension = join(root, 'extension');
  await buildTestExtension(namespace, extension);
  const provider = await writeProvider({
    path: 'unused.txt',
    text: '',
    planAction: chromeScript(sites, TYPED),
    finalText: '甲页面已加一并填好名字，乙页面的记录已删除。',
  });
  const before = new Set(await credentials(pairingPrefix));
  let browser: Awaited<ReturnType<typeof launchChrome>> | undefined;
  let aura: Awaited<ReturnType<typeof launch>> | undefined;
  try {
    aura = await launch(root);
    const { page } = aura;

    // Turning Chrome control on registers this checkout's relay for the test extension only.
    expect(await status(page)).toMatchObject({ enabled: false, registered: false });
    expect(await setEnabled(page, true)).toMatchObject({ enabled: true, registered: true });
    const manifestPath = await registeredManifest(host);
    expect(manifestPath).not.toBeNull();
    expect(resolve(manifestPath!).toLowerCase().startsWith(resolve(root).toLowerCase())).toBe(true);
    const manifest = JSON.parse(await readFile(manifestPath!, 'utf8')) as Record<string, unknown>;
    expect(String(manifest.path).toLowerCase()).toBe(relay.toLowerCase());
    expect(manifest).toMatchObject({
      name: host,
      type: 'stdio',
      allowed_origins: [`chrome-extension://${TEST_EXTENSION_ID}/`],
    });

    // Chrome starts the relay as a program of its own; Aura asks for the pairing code.
    const code = value(await page.evaluate(() => window.aura.v1.chrome!.beginPairing()));
    browser = await launchChrome(join(root, 'chrome-profile'), extension);
    expect(browser.extensionId).toBe(TEST_EXTENSION_ID);
    const panel = await browser.context.newPage();
    await panel.goto(`chrome-extension://${TEST_EXTENSION_ID}/side-panel/index.html`);
    await expect(panel.getByText('未配对')).toBeVisible({ timeout: 60_000 });
    expect((await runningRelays()).map((path) => path.toLowerCase())).toContain(
      relay.toLowerCase(),
    );
    await panel.getByPlaceholder('配对码').fill(code.code);
    await panel.getByRole('button', { name: '配对', exact: true }).click();
    await expect(panel.getByText('已连接')).toBeVisible({ timeout: 30_000 });
    await expect.poll(async () => (await status(page)).connection).toBe('connected');
    expect((await status(page)).paired?.extension_id).toBe(TEST_EXTENSION_ID);
    const paired = (await credentials(pairingPrefix)).filter((target) => !before.has(target));
    expect(paired, 'the pairing secret is among the installation credentials').toHaveLength(1);
    await panel.screenshot({ path: info.outputPath('panel-connected.png') });

    // A read-only conversation: opening asks for the site, the first action for this turn, a
    // second site again, and the delete button each time.
    const workspace = join(root, 'workspace');
    await mkdir(workspace);
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'L14 Chrome',
      text: '在甲页面加一并填上名字，再到乙页面删除全部记录',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const seen = new Set<string>();
    const opening = await answerNext(page, session, seen, 'allow_once');
    expect(opening.decisions).toEqual(['allow_once', 'deny']);
    const acting = await answerNext(page, session, seen, 'allow_for_current_turn');
    expect(acting.capabilities).toContain('chrome_interaction');
    expect(acting.decisions).toContain('allow_for_current_turn');
    const leaving = await answerNext(page, session, seen, 'allow_once');
    expect(leaving.decisions).toEqual(['allow_once', 'deny']);
    const erasing = await answerNext(page, session, seen, 'allow_once');
    expect(erasing.capabilities).toContain('chrome_sensitive_action');
    expect(erasing.decisions).toEqual(['allow_once', 'deny']);
    await finished(page, session);
    expect(provider.errors).toEqual([]);
    // Within its idle minute the extension still holds the tab it acted in.
    expect(await extensionHolds(browser.worker, sites.b)).toBe(true);

    // What the pages saw: the click, the text, and the delete on the second site.
    expect(sites.hits).toContain('added');
    expect(sites.hits).toContain(`typed:${TYPED}`);
    expect(sites.hits).toContain('erased');
    const tab = browser.context.pages().find((candidate) => candidate.url() === sites.b);
    expect(tab, 'the tab the model opened is on the second site').toBeDefined();
    await expect(tab!.locator('#state')).toHaveText('已删除');
    await tab!.screenshot({ path: info.outputPath('shared-tab.png') });
    const group = await browser.worker.evaluate(async (url) => {
      const api = (globalThis as unknown as { chrome: ChromeApi }).chrome;
      const [opened] = await api.tabs.query({ url });
      const groups = await api.tabGroups.query({});
      return {
        titles: groups.map((value) => value.title),
        inGroup: groups.some((value) => value.id === opened?.groupId),
      };
    }, sites.b);
    expect(group).toEqual({ titles: ['Aura'], inGroup: true });
    const shared = await shares(page, session);
    expect(shared.tabs.map((item) => item.url)).toEqual([sites.b]);
    // The side panel lists the shared page with a stop of its own, and stops everything below.
    const listed = panel.locator('li.share');
    await expect(listed).toHaveCount(1);
    await expect(listed.getByRole('button', { name: '停止共享', exact: true })).toBeVisible();
    await expect(panel.getByRole('button', { name: '全部停止', exact: true })).toBeVisible();
    await panel.screenshot({ path: info.outputPath('panel-shares.png') });

    // Stopping in Aura ends the conversation's share and Chrome lets go of the tab.
    await page.locator('.session-row').first().click();
    const strip = page.getByRole('status', { name: 'Chrome' });
    await expect(strip).toContainText('Chrome · 1 个标签页');
    await page.screenshot({ path: info.outputPath('aura-chrome-strip.png') });
    await strip.getByRole('button', { name: '停止', exact: true }).click();
    await expect.poll(async () => (await shares(page, session)).tabs.length).toBe(0);
    await expect(strip).toHaveCount(0);
    await expect.poll(() => extensionHolds(browser!.worker, sites.b)).toBe(false);

    // Forgetting the extension deletes its secret; the panel asks for a code again.
    value(await page.evaluate(() => window.aura.v1.chrome!.revoke()));
    await expect(panel.getByText('未配对')).toBeVisible({ timeout: 30_000 });
    expect(await credentials(pairingPrefix)).toEqual([...before]);

    // Turning Chrome control off removes Chrome's registration and the manifest.
    expect(await setEnabled(page, false)).toMatchObject({ enabled: false, registered: false });
    expect(await registeredManifest(host)).toBeNull();
    await expect(access(manifestPath!)).rejects.toThrow();
  } finally {
    await browser?.context.close();
    await aura?.app.close();
    await removeRegistration(host);
    for (const target of await credentials(pairingPrefix).catch(() => []))
      if (!before.has(target)) await deleteCredential(target);
    await provider.close();
    await sites.close();
    await cleanup(root);
  }
  // Chrome closed, so the relay it started ends too.
  await expect
    .poll(
      async () =>
        (await runningRelays()).some((path) => path.toLowerCase() === relay.toLowerCase()),
      {
        timeout: 15_000,
      },
    )
    .toBe(false);
});

/**
 * Whether the extension itself has the tab's debugger. Chrome's own "attached" also counts
 * Playwright's connection, so the extension attaches once more: Chrome refuses that only while it
 * already has the tab, and a probe that got through is let go at once.
 */
async function extensionHolds(worker: Worker, url: string): Promise<boolean> {
  return worker.evaluate(async (address) => {
    const api = (globalThis as unknown as { chrome: ChromeApi }).chrome;
    const [tab] = await api.tabs.query({ url: address });
    if (tab?.id === undefined) throw new Error(`no tab at ${address}`);
    try {
      await api.debugger.attach({ tabId: tab.id }, '1.3');
    } catch (error) {
      if (/already attached/iu.test(String(error))) return true;
      throw error;
    }
    await api.debugger.detach({ tabId: tab.id });
    return false;
  }, url);
}

/** The part of Chrome's extension API the checks above use in the service worker. */
interface ChromeApi {
  tabs: { query(filter: { url?: string }): Promise<Array<{ id?: number; groupId?: number }>> };
  tabGroups: { query(filter: object): Promise<Array<{ id: number; title?: string }>> };
  debugger: {
    attach(target: { tabId: number }, version: string): Promise<void>;
    detach(target: { tabId: number }): Promise<void>;
  };
}
