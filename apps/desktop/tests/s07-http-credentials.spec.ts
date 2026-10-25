import { expect, test, type Page } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdir, mkdtemp, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { armReadOnlyWrite } from './s06/setup';
import { answer, finished, history, launch, pending, submit } from './s06/harness';
import { httpService } from './s07/http-service';
import { networkProvider } from './s07/provider';

async function httpTarget(page: Page) {
  return page.evaluate(async () => {
    const api = window.aura.v1;
    const settings = await api.readAgentToolSettings({});
    if (!settings.ok) throw new Error('The real tool settings are unavailable');
    const http = settings.value.tools.find((tool) => tool.name === 'http.request');
    if (!http) throw new Error('The builtin HTTP tool is missing');
    const detail = await api.refreshToolProjection({
      projection_kind: 'tool_definition',
      projection_scope: { type: 'tool_definition', tool_id: http.tool_id },
    });
    if (!detail.ok || detail.value.projection_kind !== 'tool_definition')
      throw new Error('The real HTTP definition projection is unavailable');
    const instances = detail.value.payload.versions.flatMap((version) => version.instances);
    const instance = instances.find((value) => value.credential_slot_binding != null);
    const slot = instance?.credential_slot_binding;
    if (!instance || !slot) throw new Error('The HTTP credential slot is missing');
    return {
      tool_instance_id: instance.tool_instance_id,
      expected_instance_revision: instance.entity_revision,
      credential_slot_key: slot.slot_key,
      placement: slot.placement,
      workspace_id: null,
      session_id: null,
    };
  });
}

async function totalAttempts(root: string) {
  const files = (await readdir(root, { recursive: true })).filter((file) =>
    file.endsWith('aura-v2.db'),
  );
  expect(files).toHaveLength(1);
  const db = new DatabaseSync(join(root, files[0]!), { readOnly: true });
  try {
    return Number(db.prepare('SELECT count(*) AS n FROM execution_attempts').get()!.n);
  } finally {
    db.close();
  }
}

async function clearOwnedCredential(page: Page, displayName: string, ownedCredentialId?: string) {
  const target = await httpTarget(page);
  const read = await page.evaluate(
    async (request) => window.aura.v1.listToolCredentials(request),
    target,
  );
  if (!read.ok) throw new Error('The owned credential cleanup could not read its metadata');
  const own = read.value.credentials.find((record) =>
    ownedCredentialId
      ? record.credential_id === ownedCredentialId
      : record.display_name === displayName,
  );
  if (own && own.state !== 'revoked') {
    const clear = await page.evaluate(
      async (request) => window.aura.v1.clearToolCredential(request),
      { target, credential_id: own.credential_id, expected_row_revision: own.row_revision },
    );
    expect(clear.ok, 'the isolated synthetic secret must be cleared').toBe(true);
  }
}

// eslint-disable-next-line no-empty-pattern -- real Electron, Core, Credential Manager and local services.
test('S07 HTTP credentials UI binds an allowed website and refuses it before dispatch after an edit', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s07-http-credential-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const http = await httpService();
  const other = await httpService();
  const provider = await networkProvider();
  provider.setAction('http.request', { url: http.url });
  const secret = `s07-ui-${createHash('sha256').update(root).digest('hex')}`;
  const displayName = 'S07 isolated HTTP credential';
  const { app, page } = await launch(root);
  let ownedCredentialId: string | undefined;
  let credentialStarted = false;
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'S07 HTTP credentials',
      text: '准备本地 API',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await answer(page, session, 'deny');
    await finished(page, session);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '工具', exact: true })
      .click();
    await page.getByRole('button', { name: '管理 HTTP 凭据与允许网站', exact: true }).click();
    const details = page.getByRole('region', { name: 'HTTP 工具凭据', exact: true });
    const manager = details.getByRole('region', { name: '管理工具凭据', exact: true });
    await manager.getByLabel('名称', { exact: true }).fill(displayName);
    await manager.getByLabel('密钥', { exact: true }).fill(secret);
    await manager.getByLabel('允许网站（每行一个来源）').fill(http.url);
    credentialStarted = true;
    await manager.getByRole('button', { name: '创建凭据', exact: true }).click();
    await expect(manager.getByRole('status')).toContainText('变更已提交');
    await expect(manager.getByLabel('密钥', { exact: true })).toHaveValue('');
    const created = await page.evaluate(
      async (target) => window.aura.v1.listToolCredentials(target),
      await httpTarget(page),
    );
    expect(created.ok).toBe(true);
    if (!created.ok) throw new Error('Created credential metadata is unavailable');
    const owned = created.value.credentials.find((record) => record.display_name === displayName);
    expect(owned).toBeDefined();
    ownedCredentialId = owned!.credential_id;
    expect(owned!.allowed_origins).toEqual([http.url]);
    await details.getByRole('button', { name: '读取可用 Credential', exact: true }).click();
    await details.getByLabel('Credential').selectOption(ownedCredentialId);
    await details.getByRole('button', { name: '绑定所选 Credential', exact: true }).click();
    await expect(details.getByRole('status').first()).toContainText('Credential 绑定已提交');
    await page.getByRole('button', { name: '返回会话', exact: true }).click();
    provider.setAction('http.request', { url: http.url, credential: 'api_key' });
    await submit(page, session, '使用绑定凭据调用本地 API');
    await pending(page, session);
    expect(http.requests).toHaveLength(0);
    await page
      .locator('section[aria-label="当前任务审批"]')
      .getByRole('button', { name: '本会话允许', exact: true })
      .click();
    await finished(page, session);
    expect(http.requests).toHaveLength(1);
    expect(http.requests[0]!.authorization).toBe(`Bearer ${secret}`);
    expect(JSON.stringify(provider.requests)).not.toContain(secret);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '工具', exact: true })
      .click();
    await page.getByRole('button', { name: '管理 HTTP 凭据与允许网站', exact: true }).click();
    await manager.getByRole('button', { name: '读取凭据管理状态', exact: true }).click();
    await manager.getByLabel('操作', { exact: true }).selectOption('origins');
    await manager.getByLabel('凭据', { exact: true }).selectOption(ownedCredentialId);
    await manager.getByLabel('允许网站（每行一个来源）').fill(other.url);
    await manager.getByRole('button', { name: '保存允许网站', exact: true }).click();
    await expect(manager.getByRole('status')).toContainText('变更已提交');
    await expect(manager).toContainText(other.url);
    await page.getByRole('button', { name: '返回会话', exact: true }).click();
    const attempts = await totalAttempts(root);
    await submit(page, session, '再次调用相同 API');
    await finished(page, session);
    expect(http.requests).toHaveLength(1);
    expect(other.requests).toHaveLength(0);
    expect(await totalAttempts(root)).toBe(attempts);
    const view = await history(page, session);
    expect(view.active?.pending_approval_ids ?? []).toEqual([]);
    expect(JSON.stringify(provider.requests.at(-1)?.messages)).toContain(
      'tool.resource.http_credential_origin_denied',
    );
    expect(JSON.stringify(provider.requests)).not.toContain(secret);
    expect(provider.errors).toEqual([]);
    await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  } catch (error) {
    await info.attach('credential-ui-failure-state', {
      body: await page
        .locator('body')
        .innerText()
        .catch(() => 'Page unavailable'),
      contentType: 'text/plain',
    });
    throw error;
  } finally {
    try {
      if (credentialStarted) await clearOwnedCredential(page, displayName, ownedCredentialId);
    } finally {
      await app.close();
      await provider.close();
      await http.close();
      await other.close();
    }
  }
});
