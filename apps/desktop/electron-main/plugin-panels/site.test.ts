// @vitest-environment node
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import {
  entryUrl,
  panelCsp,
  panelOrigin,
  panelResponse,
  requestAllowed,
  type PanelSite,
} from './site.js';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

const site = (folder: string, network: string[] = []): PanelSite => ({
  plugin: 'usage-panel',
  title: '本会话用量',
  folder,
  entry: 'panel/index.html',
  network,
  capabilities: ['session.read'],
});

test('a panel has an origin of its own, and its entry is a page of it', () => {
  expect(panelOrigin('usage-panel')).toBe('aura-plugin://usage-panel');
  expect(entryUrl(site('C:\\p'))).toBe('aura-plugin://usage-panel/index.html');
  expect(entryUrl({ ...site('C:\\p'), entry: 'panel/app/main page.html' })).toBe(
    'aura-plugin://usage-panel/app/main%20page.html',
  );
});

// What leaves the page: its own files, and only the origins its plugin declared.
test('a panel reaches only its own files and the origins it declared', () => {
  const declared = site('C:\\p', ['https://api.example.com', 'wss://push.example.com:8443/']);
  for (const url of [
    'aura-plugin://usage-panel/lib/app.js',
    'https://api.example.com/v1/usage?x=1',
    'https://API.example.com/',
    'wss://push.example.com:8443/socket',
  ])
    expect(requestAllowed(url, declared), url).toBe(true);
  for (const url of [
    'aura-plugin://other-plugin/index.html',
    'http://api.example.com/',
    'https://api.example.com:444/',
    'https://evil.example.com/',
    'https://api.example.com.evil.com/',
    'wss://api.example.com/',
    'file:///C:/Windows/win.ini',
    'aura-app://renderer/index.html',
    'data:text/html,hi',
    'not a url',
  ])
    expect(requestAllowed(url, declared), url).toBe(false);
});

test('the page runs under a policy that allows no inline script and only declared connections', () => {
  const policy = panelCsp({ network: ['https://api.example.com/'] });
  expect(policy).toContain("default-src 'none'");
  expect(policy).toContain("script-src 'self';");
  expect(policy).not.toMatch(/script-src[^;]*unsafe/u);
  expect(policy).toContain("connect-src 'self' https://api.example.com;");
  for (const part of [
    "frame-src 'none'",
    "object-src 'none'",
    "base-uri 'none'",
    "form-action 'none'",
  ])
    expect(policy).toContain(part);
  expect(panelCsp({ network: [] })).toContain("connect-src 'self';");
});

test('the scheme serves the panel files with its policy, and refuses everything else', async () => {
  const folder = await mkdtemp(join(tmpdir(), 'aura-panel-site-'));
  roots.push(folder);
  await mkdir(join(folder, 'panel'));
  await writeFile(join(folder, 'panel', 'index.html'), '<p>usage</p>');
  await writeFile(join(folder, 'plugin.toml'), 'name = "usage-panel"');
  const open = site(folder);

  const page = await panelResponse(
    { method: 'GET', url: 'aura-plugin://usage-panel/index.html' },
    open,
  );
  expect(page.status).toBe(200);
  expect(await page.text()).toBe('<p>usage</p>');
  expect(page.headers.get('content-type')).toBe('text/html; charset=utf-8');
  expect(page.headers.get('content-security-policy')).toBe(panelCsp(open));
  expect(page.headers.get('x-content-type-options')).toBe('nosniff');
  expect(page.headers.get('cache-control')).toBe('no-store');

  const status = async (request: { method: string; url: string }) =>
    (await panelResponse(request, open)).status;
  // A panel that was closed serves nothing more.
  const closed = await panelResponse(
    { method: 'GET', url: 'aura-plugin://usage-panel/index.html' },
    undefined,
  );
  expect(closed.status).toBe(403);
  expect(await status({ method: 'GET', url: 'aura-plugin://other/index.html' })).toBe(403);
  expect(await status({ method: 'POST', url: 'aura-plugin://usage-panel/index.html' })).toBe(405);
  // The manifest beside panel/ is the plugin's, not the page's.
  expect(await status({ method: 'GET', url: 'aura-plugin://usage-panel/../plugin.toml' })).toBe(
    404,
  );
  expect(await status({ method: 'GET', url: 'aura-plugin://usage-panel/%2e%2e/plugin.toml' })).toBe(
    404,
  );
  expect(await status({ method: 'GET', url: 'aura-plugin://usage-panel/missing.js' })).toBe(404);
});
