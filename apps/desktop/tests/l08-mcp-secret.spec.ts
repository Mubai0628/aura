import { expect, test, type Page } from '@playwright/test';
import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { createInterface } from 'node:readline';
import { launch } from './s06/harness';
import { expectSecureSurface } from './electron-shell/renderer-api';

/** The repository's HTTP test MCP server, requiring exactly `header` on every MCP request. */
async function protectedServer(
  marker: string,
  header: string,
): Promise<{ url: string; server: ChildProcess }> {
  const repository = resolve(import.meta.dirname, '../../..');
  const server = spawn(
    join(repository, 'target/debug/aura-mcp-http-fixture.exe'),
    ['--require-auth', header, '--marker', marker],
    { stdio: ['pipe', 'pipe', 'ignore'] },
  );
  const lines = createInterface({ input: server.stdout as NodeJS.ReadableStream });
  const url = await new Promise<string>((done, fail) => {
    lines.once('line', (line) => done(line.trim()));
    server.once('error', fail);
    server.once('exit', () => fail(new Error('the HTTP test server ended before its address')));
  });
  lines.close();
  return { url, server };
}

/**
 * Whether the Credential Manager has Aura's record for one secret (its name only). A filtered
 * `/list:<name>` never finds these entries (they are listed as `LegacyGeneric:target=<name>`)
 * yet repeats the name in its header, so the whole listing is read and only an entry line counts.
 */
function stored(secretId: string): boolean {
  const listed = execFileSync('cmdkey', ['/list'], { encoding: 'utf8' });
  return listed.toLowerCase().includes(`target=aura/mcp/secret/${secretId.toLowerCase()}`);
}

async function mcpSettings(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: 'MCP', exact: true })
    .click();
  return page.getByRole('region', { name: 'MCP 设置' });
}

// eslint-disable-next-line no-empty-pattern -- real Electron/Core and the Windows Credential Manager (Aura's own record for a fresh secret, removed by the product at the end); the repo's HTTP MCP server on 127.0.0.1.
test('L08 B5 Credential Manager: an HTTP server signs in with a secret that never leaves Aura', async ({}, info) => {
  test.skip(
    process.env.AURA_E2E_CREDENTIAL_MANAGER !== '1',
    'writes the Windows Credential Manager; runs only with the owner OK',
  );
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l08-mcp-secret-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const secret = `e2e-${randomBytes(18).toString('hex')}`;
  const marker = join(root, 'secret-calls.txt');
  const { url, server } = await protectedServer(marker, `authorization:Bearer ${secret}`);
  const { app, page } = await launch(root);
  let secretId: string | undefined;
  try {
    await expectSecureSurface(page);
    const settings = await mcpSettings(page);
    await settings.getByRole('button', { name: '添加服务器' }).click();
    const form = settings.getByRole('form', { name: '添加 MCP 服务器' });
    await form.getByLabel('服务器 ID').fill('web');
    await form.getByLabel('显示名').fill('测试密钥');
    await form.getByLabel('HTTP 地址（Streamable HTTP）').check();
    await form.getByLabel('地址', { exact: true }).fill(url);
    await form.getByLabel('密钥（Authorization: Bearer）').check();
    await form.getByLabel('密钥', { exact: true }).fill(secret);
    await form.getByRole('button', { name: '保存' }).click();
    const list = settings.getByRole('list', { name: 'MCP 服务器' });
    await list.getByRole('button', { name: '查看并信任' }).click();
    await settings
      .getByRole('alertdialog', { name: '信任 测试密钥' })
      .getByRole('button', { name: '信任并启用' })
      .click();
    await expect(list).toContainText('可用 · 10 个工具', { timeout: 60_000 });
    const registry = await page.evaluate(async () => window.aura.v1.listMcpServers({}));
    expect(JSON.stringify(registry)).not.toContain(secret);
    const transport = registry.ok ? registry.value.servers[0]?.config.transport : undefined;
    secretId =
      transport?.type === 'http' && transport.auth?.type === 'secret'
        ? transport.auth.secret_id
        : undefined;
    expect(secretId).toMatch(/^crd_/u);
    expect(stored(secretId as string)).toBe(true);
    expect((await readFile(marker, 'utf8')).includes('unauthorized')).toBe(false);
    // Removing the server removes its secret from the Credential Manager.
    await list.getByRole('button', { name: '移除' }).click();
    await settings
      .getByRole('alertdialog', { name: '移除 测试密钥' })
      .getByRole('button', { name: '移除' })
      .click();
    await expect(settings.getByText('还没有 MCP 服务器', { exact: true })).toBeVisible();
    expect(stored(secretId as string)).toBe(false);
  } finally {
    await app.close();
    server.kill();
    // A failed run still never leaves the record behind.
    if (secretId && stored(secretId))
      execFileSync('cmdkey', [`/delete:Aura/mcp/secret/${secretId}`]);
  }
});
