import { expect, test, type Page, type Worker } from '@playwright/test';
import { access, mkdir, mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { isAbsolute, join, relative, resolve } from 'node:path';
import type { ProviderRequest } from './s06-file-write-provider.mjs';
import { writeProvider } from './s06-file-write-provider.mjs';
import { history, launch } from './s06/harness';
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
  startSites,
} from './l14/chrome';
import { refOf } from './l14/model';
import { throughEntry } from './tool-entry';

// Real Electron and Core, the relay as Chrome starts it, and the test build of the extension in
// Chrome for Testing (Playwright's Chromium); only the model is scripted. The user picks a tab of
// their own from the composer's "@" list: it is shared with the conversation and read for the
// message, which attaches what it showed as a file of the conversation; the model reads that file
// and clicks in the tab by the name the file gives it, and the user's Stop on the conversation's
// line takes it back. Chrome control writes this development namespace's host registration under
// HKCU and its pairing secret to the Credential Manager; both are removed.

type Envelope<T> = { ok: true; value: T } | { ok: false; error: unknown };
const value = <T>(result: Envelope<T>): T => {
  if (!result.ok) throw new Error(JSON.stringify(result.error));
  return result.value;
};

/** The newest user message as the model reads it. */
function userText(request: ProviderRequest): string {
  const user = request.messages
    .filter((message) => (message as { role?: string }).role === 'user')
    .at(-1) as { content?: unknown } | undefined;
  const content = user?.content;
  if (typeof content === 'string') return content;
  return Array.isArray(content)
    ? content.map((part) => String((part as { text?: string }).text ?? '')).join('')
    : '';
}

/** The path of the file the message attaches, as the model is told it. */
function attachedPath(text: string): string | undefined {
  const lead = 'external file `';
  const start = text.indexOf(lead);
  if (start < 0) return undefined;
  const from = start + lead.length;
  return text.slice(from, text.indexOf('`', from));
}

/** What the model's last read gave it. */
function readText(request: ProviderRequest): string | undefined {
  const message = request.messages
    .filter((one) => (one as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  if (message === undefined) return undefined;
  const result = JSON.parse(message.content) as {
    outcome?: { payload?: { value?: { text?: string } } };
  };
  return result.outcome?.payload?.value?.text;
}

/** The model reads the tab the message attaches, then clicks in it by the name it is given. */
function clickInHandedTab(request: ProviderRequest) {
  if (request.step === 0) {
    const path = attachedPath(userText(request));
    if (path === undefined) throw new Error('the message attached no file');
    return {
      tool: 'fs.read',
      arguments: { path, range: { range_kind: 'full' }, explicit_encoding: null },
    };
  }
  if (request.step !== 1) return { tool: null, arguments: {} };
  const tab = readText(request);
  if (tab === undefined || !tab.includes('Chrome tab "'))
    throw new Error(`the attached file is not the tab: ${String(tab).slice(0, 400)}`);
  return throughEntry({
    tool: 'chrome.click',
    arguments: { tab: 't1', ref: refOf(tab, 'button', '加一') },
  });
}

/** Answers every approval of the session until its turn completes. */
async function untilCompleted(page: Page, session: string) {
  const answered: string[] = [];
  await expect
    .poll(
      async () => {
        const view = await history(page, session);
        for (const id of view.active?.pending_approval_ids ?? []) {
          if (answered.includes(id)) continue;
          answered.push(id);
          const approval = value(
            await page.evaluate(async (request) => window.aura.v1.getAgentApproval(request), {
              session_id: session,
              approval_id: id,
            }),
          );
          const allowed = approval.approval.allowed_decisions;
          const decision = (
            ['allow_once', 'allow_for_current_turn', 'allow_session'] as const
          ).find((one) => allowed.includes(one));
          if (decision === undefined) throw new Error(`no allowing decision: ${allowed.join()}`);
          await page.evaluate(async (request) => window.aura.v1.resolveAgentApproval(request), {
            session_id: session,
            approval_id: id,
            expected_revision: approval.approval.revision,
            decision,
          });
        }
        return view.active?.turn.state.state ?? view.turns.at(-1)?.turn.state.state;
      },
      { timeout: 120_000 },
    )
    .toBe('completed');
  return answered;
}

/** A conversation on the scripted model, with nothing sent yet. */
async function conversation(page: Page, workspaceRoot: string, providerUrl: string) {
  return page.evaluate(
    async ({ selectedRoot, url }) => {
      const api = window.aura.v1;
      const fail = (what: string, result: unknown) => {
        throw new Error(`${what}: ${JSON.stringify(result)}`);
      };
      const workspace = await api.createWorkspace({
        display_name: 'L21 Chrome @',
        selected_root: selectedRoot,
      });
      if (!workspace.ok) fail('workspace', workspace);
      const workspaceId = workspace.ok ? workspace.value.workspace.workspace_id : '';
      const session = await api.createSession({ workspace_id: workspaceId, title: 'L21 Chrome @' });
      if (!session.ok) fail('session', session);
      const sessionId = session.ok ? session.value.session_id : '';
      const settings = await api.getModelSettings();
      if (!settings.ok) fail('settings', settings);
      const provider = await api.createModelProvider({
        expected_revision: settings.ok ? settings.value.models_revision : '0',
        input: { display_name: 'L21 model', source: { type: 'custom', catalog_provider_id: null } },
      });
      if (!provider.ok) fail('provider', provider);
      const endpoint = await api.createModelEndpoint({
        expected_revision: provider.ok ? provider.value.revision : '0',
        provider_id: provider.ok ? provider.value.value.id : '',
        display_name: 'L21 endpoint',
        api_format: 'open_ai_chat_completions',
        base_url: url,
        discovery_mode: { type: 'disabled' },
        chat_output_token_field: 'max_tokens',
        chat_instruction_role: 'system',
        auth_scheme: 'none',
        openai_organization_id: null,
        openai_project_id: null,
        anthropic_workspace_id: null,
        timeouts: {
          connect_ms: 5000,
          first_event_ms: 10_000,
          stream_idle_ms: 10_000,
          total_ms: 60_000,
        },
        max_parallel_model_requests: 2,
      });
      if (!endpoint.ok) fail('endpoint', endpoint);
      const profile = await api.createModelProfile({
        expected_revision: endpoint.ok ? endpoint.value.revision : '0',
        input: {
          endpoint_id: endpoint.ok ? endpoint.value.value.id : '',
          provider_model_name: 'fixture',
          display_name: 'L21 fixture model',
          notes: null,
          context_window_tokens: 256000,
        },
      });
      if (!profile.ok) fail('profile', profile);
      const profileId = profile.ok ? profile.value.value.id : '';
      const enabled = await api.updateModelProfile({
        expected_revision: profile.ok ? profile.value.revision : '0',
        model_profile_id: profileId,
        display_name: 'L21 fixture model',
        notes: null,
        enabled: true,
        request_defaults: {
          max_output_tokens: 4096,
          temperature: null,
          top_p: null,
          chat_output_token_field: null,
          chat_instruction_role: null,
        },
        historical_snapshot_max_tokens: 1024,
      });
      if (!enabled.ok) fail('profile enable', enabled);
      const binding = await api.setSessionModelBinding({
        session_id: sessionId,
        expected_revision: '0',
        model_profile_id: profileId,
      });
      if (!binding.ok) fail('binding', binding);
      return sessionId;
    },
    { selectedRoot: workspaceRoot, url: providerUrl },
  );
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
  if (isAbsolute(rel) || rel.startsWith('..') || !rel.startsWith('aura-l21-chrome-'))
    throw new Error(`Refusing to remove a non-fixture directory: ${root}`);
  await rm(root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
}

// eslint-disable-next-line no-empty-pattern -- real processes on isolated data; see above.
test('L21 a Chrome tab picked from "@" is shared with the conversation, the model acts in it, and Stop takes it back', async ({}, info) => {
  test.setTimeout(480_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l21-chrome-'));
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
    planAction: clickInHandedTab,
    finalText: '已经在你的标签页里点了加一。',
  });
  const before = new Set(await credentials(pairingPrefix));
  let browser: Awaited<ReturnType<typeof launchChrome>> | undefined;
  let aura: Awaited<ReturnType<typeof launch>> | undefined;
  try {
    aura = await launch(root);
    const { page } = aura;
    const revision =
      value(await page.evaluate(() => window.aura.v1.chrome!.status())).revision ?? 0;
    value(
      await page.evaluate(async (request) => window.aura.v1.chrome!.setEnabled(request), {
        enabled: true,
        expected_revision: revision,
      }),
    );
    const code = value(await page.evaluate(() => window.aura.v1.chrome!.beginPairing()));
    browser = await launchChrome(join(root, 'chrome-profile'), extension);
    expect(browser.extensionId).toBe(TEST_EXTENSION_ID);
    const panel = await browser.context.newPage();
    await panel.goto(`chrome-extension://${TEST_EXTENSION_ID}/side-panel/index.html`);
    await expect(panel.getByText('未配对')).toBeVisible({ timeout: 60_000 });
    await panel.getByPlaceholder('配对码').fill(code.code);
    await panel.getByRole('button', { name: '配对', exact: true }).click();
    await expect(panel.getByText('已连接')).toBeVisible({ timeout: 30_000 });

    // The user's own tab, on a site of theirs.
    const tab = await browser.context.newPage();
    await tab.goto(sites.a);
    await expect(tab.getByText('甲页面').first()).toBeVisible();

    const workspace = join(root, 'workspace');
    await mkdir(workspace);
    const session = await conversation(page, workspace, provider.url);
    await page.locator('.session-row').filter({ hasText: 'L21 Chrome @' }).first().click();
    const box = page.getByRole('textbox', { name: '输入消息' });
    await box.click();
    await page.keyboard.type('@');
    // Only the ordinary pages are offered: not the extension's own page, not a blank one.
    const offered = page.getByRole('listbox', { name: 'Chrome 标签页' });
    await expect(offered).toBeVisible({ timeout: 15_000 });
    await expect(offered.getByRole('option')).toHaveText([`甲页面${sites.a}`]);
    await page.screenshot({ path: info.outputPath('at-menu-chrome.png') });
    await offered.getByRole('option').first().click();
    await expect(page.getByRole('group', { name: '引用' })).toContainText('甲页面');
    await page.keyboard.type('点一下加一');
    await page.keyboard.press('Enter');
    const approvals = await untilCompleted(page, session);
    expect(provider.errors).toEqual([]);

    // The message attached the tab as a file, and the share loaded Chrome's tools at once.
    const first = provider.requests.find((request) => request.step === 0)!;
    expect(attachedPath(userText(first)), 'the message attached the tab').toBeDefined();
    expect(
      (first.tools as { function?: { name?: string } }[] | undefined)?.some((tool) =>
        tool.function?.name?.startsWith('chrome'),
      ),
    ).toBe(true);
    // The file is the page as it was, named as Chrome's tools take it.
    const carried = readText(provider.requests.find((request) => request.step === 1)!);
    expect(carried).toContain('Chrome tab "甲页面"');
    expect(carried).toContain('as tab `t1`');
    expect(carried).toContain(sites.a);
    expect(carried).toMatch(/button "加一" \[ref=e\d+\]/u);
    // The click reached the page, which is shared with the conversation and in the Aura group.
    expect(sites.hits).toContain('added');
    await expect(tab.locator('#count')).toHaveText('1');
    const shared = value(
      await page.evaluate(async (id) => window.aura.v1.chrome!.shares({ session_id: id }), session),
    );
    expect(shared.tabs.map((item) => [item.tab, item.url])).toEqual([['t1', sites.a]]);
    const group = await browser.worker.evaluate(async (url) => {
      const api = (globalThis as unknown as { chrome: ChromeApi }).chrome;
      const [opened] = await api.tabs.query({ url });
      const groups = await api.tabGroups.query({});
      return {
        titles: groups.map((one) => one.title),
        inGroup: groups.some((one) => one.id === opened?.groupId),
      };
    }, sites.a);
    expect(group).toEqual({ titles: ['Aura'], inGroup: true });
    await expect(panel.locator('li.share')).toHaveCount(1);
    await info.attach('approvals', {
      body: JSON.stringify(approvals),
      contentType: 'application/json',
    });

    // Stop on the conversation's line takes the tab back, and Chrome lets go of it.
    const strip = page.getByRole('status', { name: 'Chrome' });
    await expect(strip).toContainText('Chrome · 1 个标签页');
    await strip.getByRole('button', { name: '停止', exact: true }).click();
    await expect
      .poll(
        async () =>
          value(
            await page.evaluate(
              async (id) => window.aura.v1.chrome!.shares({ session_id: id }),
              session,
            ),
          ).tabs.length,
      )
      .toBe(0);
    await expect.poll(() => extensionHolds(browser!.worker, sites.a)).toBe(false);
    await expect(tab.locator('#count')).toHaveText('1');

    value(await page.evaluate(() => window.aura.v1.chrome!.revoke()));
    const off = value(await page.evaluate(() => window.aura.v1.chrome!.status())).revision ?? 0;
    value(
      await page.evaluate(async (request) => window.aura.v1.chrome!.setEnabled(request), {
        enabled: false,
        expected_revision: off,
      }),
    );
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
