import { expect, test, type Page } from '@playwright/test';
import { ok as assert } from 'node:assert/strict';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch, answer, finished, submit, pending } from './s06/harness';
import { armReadOnlyWrite } from './s06/setup';
import { httpService } from './s07/http-service';
import { networkProvider } from './s07/provider';
import { seedPreviousHttp } from './s09/upgrade-http';

async function target(page: Page) {
  return page.evaluate(async () => {
    const api = window.aura.v1;
    const settings = await api.readAgentToolSettings({});
    if (!settings.ok) throw new Error('Settings unavailable');
    const tool = settings.value.tools.find((t) => t.name === 'http.request');
    if (!tool) throw new Error('HTTP tool unavailable');
    const r = await api.refreshToolProjection({
      projection_kind: 'tool_definition',
      projection_scope: { type: 'tool_definition', tool_id: tool.tool_id },
    });
    if (!r.ok || r.value.projection_kind !== 'tool_definition')
      throw new Error('HTTP projection unavailable');
    const instance = r.value.payload.versions.at(-1)?.instances[0];
    const slot = instance?.credential_slot_binding;
    if (!instance || !slot) throw new Error('Credential owner unavailable');
    return {
      request: {
        tool_instance_id: instance.tool_instance_id,
        expected_instance_revision: instance.entity_revision,
        credential_slot_key: slot.slot_key,
        placement: slot.placement,
        workspace_id: null,
        session_id: null,
      },
      bound: slot.bound_credential,
      rebind: slot.rebind_required_reason,
    };
  });
}
async function openSettings(page: Page) {
  await page.getByRole('button', { name: '设置', exact: true }).click();
  await page
    .locator('.settings-navigation')
    .getByRole('button', { name: '工具', exact: true })
    .click();
}

// eslint-disable-next-line no-empty-pattern -- isolated real Electron/Core and synthetic owned Credential.
test('S09 builtin upgrade preserves the UI configuration and HTTP credential website without rebinding', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s09-upgrade-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const http = await httpService();
  const provider = await networkProvider();
  provider.setAction('http.request', { url: http.url });
  let current = await launch(root);
  let appOpen = true;
  let credentialId: string | undefined;
  let cleanupTarget: Awaited<ReturnType<typeof target>> | undefined;
  const displayName = 'S09 isolated upgrade credential';
  const secret = 's09-synthetic-local-api-key';
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'S09 upgrade',
      text: '准备本地服务',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await answer(current.page, session, 'deny');
    await finished(current.page, session);
    await openSettings(current.page);
    // HTTP tools load by default, so the user's own setting to carry over is switching it off.
    await current.page.getByLabel('http.request 默认加载', { exact: true }).uncheck();
    await current.page.getByRole('button', { name: '保存配置', exact: true }).click();
    await expect(current.page.getByText('工具配置已保存。', { exact: true })).toBeVisible();
    await current.page
      .getByRole('button', { name: '管理 HTTP 凭据与允许网站', exact: true })
      .click();
    const details = current.page.getByRole('region', { name: 'HTTP 工具凭据', exact: true });
    const manager = details.getByRole('region', { name: '管理工具凭据', exact: true });
    await manager.getByLabel('名称', { exact: true }).fill(displayName);
    await manager.getByLabel('密钥', { exact: true }).fill(secret);
    await manager.getByLabel('允许网站（每行一个来源）').fill(http.url);
    await manager.getByRole('button', { name: '创建凭据', exact: true }).click();
    await expect(manager.getByRole('status')).toContainText('变更已提交');
    const before = await target(current.page);
    cleanupTarget = before;
    const list = await current.page.evaluate(
      async (t) => window.aura.v1.listToolCredentials(t),
      before.request,
    );
    if (!list.ok) throw new Error('Credential metadata unavailable');
    credentialId = list.value.credentials.find(
      (r) => r.display_name === displayName,
    )!.credential_id;
    await details.getByRole('button', { name: '读取可用 Credential', exact: true }).click();
    await details.getByLabel('Credential').selectOption(credentialId);
    await details.getByRole('button', { name: '绑定所选 Credential', exact: true }).click();
    await expect(details.getByRole('status').first()).toContainText('Credential 绑定已提交');
    const bound = await target(current.page);
    cleanupTarget = bound;
    expect(bound.bound?.credential_id).toBe(credentialId);
    await current.app.close();
    appOpen = false;
    const previousId = await seedPreviousHttp(root, before.request.tool_instance_id);
    current = await launch(root);
    appOpen = true;
    await openSettings(current.page);
    await expect(
      current.page.getByLabel('http.request 默认加载', { exact: true }),
    ).not.toBeChecked();
    await current.page
      .getByRole('button', { name: '管理 HTTP 凭据与允许网站', exact: true })
      .click();
    const after = await target(current.page);
    cleanupTarget = after;
    expect(after.request.tool_instance_id).toBe(before.request.tool_instance_id);
    expect(after.request.tool_instance_id).not.toBe(previousId);
    expect(after.bound).toEqual(bound.bound);
    expect(after.request.placement).toEqual(before.request.placement);
    expect(after.rebind).toBeUndefined();
    const carried = await current.page.evaluate(
      async (t) => window.aura.v1.listToolCredentials(t),
      after.request,
    );
    if (!carried.ok) throw new Error('Carried credential metadata unavailable');
    expect(
      carried.value.credentials.find((r) => r.credential_id === credentialId)?.allowed_origins,
    ).toEqual([http.url]);
    await current.page.getByRole('button', { name: '返回会话', exact: true }).click();
    provider.setAction('http.request', { url: http.url, credential: 'api_key' });
    await submit(current.page, session, '使用沿用的凭据调用本地服务');
    await pending(current.page, session);
    await answer(current.page, session, 'allow_once');
    await finished(current.page, session);
    expect(http.requests).toHaveLength(1);
    expect(http.requests[0]!.authorization).toBe(`Bearer ${secret}`);
    expect(JSON.stringify(provider.requests)).not.toContain(secret);
    expect(provider.errors).toEqual([]);
    await info.attach('isolated-upgrade-data', { body: root, contentType: 'text/plain' });
  } finally {
    try {
      if (credentialId) {
        if (!appOpen) {
          current = await launch(root);
          appOpen = true;
        }
        const t = await target(current.page).catch(() => cleanupTarget);
        assert(t, 'The owned synthetic credential has no verified cleanup target');
        const r = await current.page.evaluate(
          async (input) => window.aura.v1.listToolCredentials(input),
          t.request,
        );
        const owned = r.ok
          ? r.value.credentials.find((v) => v.credential_id === credentialId)
          : undefined;
        if (owned && owned.state !== 'revoked') {
          const clear = await current.page.evaluate(
            async (input) => window.aura.v1.clearToolCredential(input),
            {
              target: t.request,
              credential_id: owned.credential_id,
              expected_row_revision: owned.row_revision,
            },
          );
          expect(clear.ok, 'clear only the synthetic credential owned by this scenario').toBe(true);
        }
      }
    } finally {
      if (appOpen) await current.app.close();
      await provider.close();
      await http.close();
    }
  }
});
