import { _electron as electron, expect, test } from '@playwright/test';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, readdir } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import type { AuraApiV1 } from '../shared/aura-api';
import { fixtureProvider } from './r10-session-provider.mjs';
import { captureSecretProducts } from './s05-secret-products.mjs';

// eslint-disable-next-line no-empty-pattern -- Electron fixtures are created explicitly.
test('S05 Tool settings manages a real Tool Secret over the authenticated production pipe', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s05-tool-secret-'));
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const secret = `s05-ui-${createHash('sha256').update(root).digest('hex')}`;
  let authenticatedModelRequests = 0;
  const modelRequestBodies: unknown[] = [];
  const provider = await fixtureProvider({
    long: false,
    observeRequest: ({
      authorization,
      body,
    }: {
      authorization: string | undefined;
      body: unknown;
    }) => {
      expect(authorization).toBe(`Bearer ${secret}-model-rotated`);
      authenticatedModelRequests += 1;
      modelRequestBodies.push(body);
    },
  });
  const app = await electron.launch({
    args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
    env: {
      ...process.env,
      APPDATA: join(root, 'Roaming'),
      LOCALAPPDATA: root,
      AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
      AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
      AURA_E2E: '1',
      AURA_E2E_CORE_MODE: 'normal',
      AURA_P05_CLEAN_ACCEPTANCE: '1',
      ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
    },
  });
  const responses: unknown[] = [];
  let cleanup: (() => Promise<unknown>) | undefined;
  let cleanupModel: (() => Promise<unknown>) | undefined;
  try {
    const page = await app.firstWindow();
    const pageErrors: string[] = [];
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await expect(page.getByText('Aura Core 已就绪')).toBeVisible({ timeout: 30_000 });
    // A local deterministic Provider substitutes only the remote service. Actual
    // credentials, production Agent execution and transport remain in this Core.
    const modelResults = await page.evaluate(
      async ({ canary, url }) => {
        const api = (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1;
        const settings = await api.getModelSettings();
        if (!settings.ok) throw new Error('Model settings not obtained');
        const provider = await api.createModelProvider({
          expected_revision: settings.value.models_revision,
          input: {
            display_name: 'S05 isolated Model credentials',
            source: { type: 'custom', catalog_provider_id: null },
          },
        });
        if (!provider.ok) throw new Error('Isolated Provider metadata not committed');
        const input = new TextEncoder().encode(canary);
        let created;
        try {
          created = await api.createModelEndpointWithCredential(
            {
              provider_id: provider.value.value.id,
              expected_revision: provider.value.revision,
              display_name: 'S05 no-network endpoint',
              base_url: url,
              api_format: 'open_ai_chat_completions',
              auth_scheme: 'open_ai_bearer_api_key',
              chat_output_token_field: 'max_completion_tokens',
              chat_instruction_role: 'system',
              discovery_mode: { type: 'disabled' },
              max_parallel_model_requests: 1,
              timeouts: {
                connect_ms: 5000,
                first_event_ms: 10_000,
                stream_idle_ms: 10_000,
                total_ms: 30_000,
              },
            },
            input,
          );
        } finally {
          input.fill(0);
        }
        if (!created.ok)
          throw new Error(
            'Real Model Secret write not obtained: ' +
              created.error.code +
              ': ' +
              created.error.message,
          );
        const status = created.value.endpoint_projection.authentication.active_credential;
        if (!status) throw new Error('Actual Model credential receipt missing');
        const next = new TextEncoder().encode(canary + '-rotated');
        let rotated;
        try {
          rotated = await api.replaceModelCredential(
            {
              endpoint_id: status.endpoint_id,
              credential_id: status.credential_id,
              credential_slot: status.slot_name,
              expected_models_revision: created.value.models_revision,
              expected_row_revision: status.row_revision,
            },
            next,
          );
        } finally {
          next.fill(0);
        }
        if (!rotated.ok) throw new Error('Real Model Secret rotation not obtained');
        return { created, rotated, status };
      },
      { canary: secret + '-model', url: provider.url },
    );
    responses.push(modelResults);
    cleanupModel = () =>
      page.evaluate(async (receipt) => {
        const api = (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1;
        const settings = await api.getModelSettings();
        if (!settings.ok) throw new Error('Current Model revision not obtained');
        return api.clearModelCredential({
          endpoint_id: receipt.status.endpoint_id,
          credential_id: receipt.status.credential_id,
          credential_slot: receipt.status.slot_name,
          expected_models_revision: settings.value.models_revision,
          expected_row_revision: receipt.rotated.value.credential_status.row_revision,
        });
      }, modelResults);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '工具', exact: true })
      .click();
    const settings = page.locator('.tool-settings');
    // Retired the P07 Draft chain that installed a package for this check. The Tool
    // Secret target is now the Built-in http.request credential slot.
    await settings.getByRole('button', { name: '管理 HTTP 凭据与允许网站', exact: true }).click();
    const detail = settings.getByRole('region', { name: 'HTTP 工具凭据' });
    const manager = detail.getByRole('region', { name: '管理工具凭据' });
    await expect(manager).toBeVisible();
    const target = await page.evaluate(async () => {
      const api = (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1;
      const settings = await api.readAgentToolSettings({});
      if (!settings.ok) throw new Error('The real tool settings were not obtained');
      const http = settings.value.tools.find((tool) => tool.name === 'http.request');
      if (!http) throw new Error('The Built-in HTTP tool was not obtained');
      const result = await api.refreshToolProjection({
        projection_kind: 'tool_definition',
        projection_scope: { type: 'tool_definition', tool_id: http.tool_id },
      });
      if (!result.ok || result.value.projection_kind !== 'tool_definition')
        throw new Error('The actual HTTP definition projection was not obtained');
      const instance = result.value.payload.versions
        .flatMap((version) => version.instances)
        .find((value) => value.credential_slot_binding != null);
      const slot = instance?.credential_slot_binding;
      if (!instance || !slot) throw new Error('The actual declared slot was not obtained');
      return {
        tool_instance_id: instance.tool_instance_id,
        expected_instance_revision: instance.entity_revision,
        credential_slot_key: slot.slot_key,
        placement: slot.placement,
        workspace_id: null,
        session_id: null,
      };
    });
    await manager.getByLabel('名称', { exact: true }).fill('S05 专用凭据');
    await manager.getByLabel('密钥', { exact: true }).fill(secret);
    await manager.getByRole('button', { name: '创建凭据', exact: true }).click();
    await expect(manager.getByRole('status')).toContainText('变更已提交');
    await expect(manager.getByLabel('密钥', { exact: true })).toHaveValue('');
    const read = () =>
      page.evaluate(
        async (request) =>
          (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1.listToolCredentials(request),
        target,
      );
    cleanup = async () => {
      const result = await read();
      if (!result.ok) return { obtained: false };
      const own = result.value.credentials.find(
        (record) => record.display_name === 'S05 专用凭据' && record.state !== 'revoked',
      );
      if (!own || result.value.pending_credential_ids.length > 0)
        return { active: Boolean(own), pending: result.value.pending_credential_ids.length };
      return page.evaluate(
        async (input) =>
          (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1.clearToolCredential(input),
        {
          target,
          credential_id: own.credential_id,
          expected_row_revision: own.row_revision,
        },
      );
    };
    const created = await read();
    responses.push(created);
    expect(created.ok).toBe(true);
    if (!created.ok) throw new Error('Created metadata was not obtained');
    expect(created.value.credentials).toHaveLength(1);
    const record = created.value.credentials[0]!;
    await manager.getByRole('button', { name: '读取凭据管理状态' }).click();
    await expect(manager.getByLabel('操作', { exact: true })).toBeEnabled();
    await manager.getByLabel('操作', { exact: true }).selectOption('rotate');
    await manager.getByLabel('凭据', { exact: true }).selectOption(record.credential_id);
    await manager.getByLabel('密钥', { exact: true }).fill(`${secret}-rotated`);
    await manager.getByRole('button', { name: '确认轮换密钥', exact: true }).click();
    await expect
      .poll(async () => {
        const result = await read();
        return result.ok ? result.value.credentials[0]?.credential_revision : undefined;
      })
      .toBe(String(BigInt(record.credential_revision) + 1n));
    const rotated = await read();
    responses.push(rotated);
    await manager.getByLabel('操作', { exact: true }).selectOption('clear');
    await manager.getByRole('checkbox').check();
    await manager.getByRole('button', { name: '确认清除密钥', exact: true }).click();
    await expect
      .poll(async () => {
        const result = await read();
        return result.ok ? result.value.credentials[0]?.state : undefined;
      })
      .toBe('revoked');
    responses.push(await read());
    await expect(manager).toHaveAttribute('aria-busy', 'false');
    await expect(
      manager.getByRole('option', { name: 'S05 专用凭据 · 已清除', exact: true }),
    ).toBeAttached();
    await page.screenshot({ path: info.outputPath('tool-credentials.png') });
    const products = await captureSecretProducts({
      page,
      root,
      endpointId: modelResults.status.endpoint_id,
      output: info.outputPath('actual-six-products'),
      provider,
      modelRequestBodies,
      responses,
    });
    expect(authenticatedModelRequests).toBeGreaterThanOrEqual(2);
    for (const body of Object.values(products) as Buffer[]) {
      expect(body.includes(Buffer.from(secret))).toBe(false);
      expect(body.includes(Buffer.from(secret, 'utf16le'))).toBe(false);
    }
    const clearModel = cleanupModel;
    cleanupModel = undefined; // an unconfirmed clear must not be replayed by teardown
    const clearedModel = await clearModel();
    expect((clearedModel as { ok: boolean }).ok).toBe(true);
    responses.push(clearedModel);
    const recovery = await page.evaluate(async () =>
      (window as unknown as { aura: { v1: AuraApiV1 } }).aura.v1.getConfigRecoveryNotice(),
    );
    responses.push(recovery);
    expect(recovery.ok).toBe(true);
    expect(pageErrors).toEqual([]);
    const responseBytes = Buffer.from(JSON.stringify(responses));
    expect(responseBytes.includes(Buffer.from(secret))).toBe(false);
    const ns = (await readdir(join(root, 'Aura/namespaces')))[0]!;
    const logs = Buffer.concat([
      await readFile(join(root, 'Aura/logs/core.jsonl')),
      await readFile(join(root, 'Aura/namespaces', ns, 'logs/main.jsonl')),
    ]);
    expect(logs.length).toBeGreaterThan(0);
    expect(logs.includes(Buffer.from(secret))).toBe(false);
    await info.attach('actual-authenticated-responses', {
      body: responseBytes,
      contentType: 'application/json',
    });
    await info.attach('core-log', { body: logs, contentType: 'application/x-ndjson' });
    await page.screenshot({ path: info.outputPath('agent-secret-products.png') });
  } finally {
    await info.attach('local-provider-measurement', {
      body: JSON.stringify({
        authenticated_requests: authenticatedModelRequests,
        errors: provider.errors,
      }),
      contentType: 'application/json',
    });
    if (cleanupModel) {
      await info.attach('owned-model-credential-cleanup', {
        body: JSON.stringify(await cleanupModel()),
        contentType: 'application/json',
      });
    }
    if (cleanup) {
      try {
        await info.attach('owned-test-credential-cleanup', {
          body: JSON.stringify(await cleanup()),
          contentType: 'application/json',
        });
      } catch {
        await info.attach('owned-test-credential-cleanup', {
          body: 'not obtained; isolated journal evidence retained',
          contentType: 'text/plain',
        });
      }
    }
    await app.close(); // The isolated data root and failed-run evidence are intentionally retained.
    await provider.close();
  }
});
