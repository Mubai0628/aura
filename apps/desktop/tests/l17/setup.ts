import { expect, type ElectronApplication, type Locator, type Page } from '@playwright/test';
import type { PermissionMode } from '@aura/contracts/generated';

/**
 * The local scripted model at `url` is the default model of new conversations, and they start in
 * `mode`: what a plugin's chats and panels start then runs on it, as it would on the user's model.
 */
export async function scriptedDefaults(page: Page, url: string, mode: PermissionMode) {
  const result = await page.evaluate(
    async ({ url, mode }) => {
      const api = window.aura.v1;
      const settings = await api.getModelSettings();
      if (!settings.ok) return { error: 'model settings' };
      const provider = await api.createModelProvider({
        expected_revision: settings.value.models_revision,
        input: { display_name: 'L17 model', source: { type: 'custom', catalog_provider_id: null } },
      });
      if (!provider.ok) return { error: JSON.stringify(provider) };
      const endpoint = await api.createModelEndpoint({
        expected_revision: provider.value.revision,
        provider_id: provider.value.value.id,
        display_name: 'L17 local endpoint',
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
      if (!endpoint.ok) return { error: JSON.stringify(endpoint) };
      const profile = await api.createModelProfile({
        expected_revision: endpoint.value.revision,
        input: {
          endpoint_id: endpoint.value.value.id,
          provider_model_name: 'fixture',
          display_name: 'L17 fixture model',
          notes: null,
          context_window_tokens: 256000,
        },
      });
      if (!profile.ok) return { error: JSON.stringify(profile) };
      const enabled = await api.updateModelProfile({
        expected_revision: profile.value.revision,
        model_profile_id: profile.value.value.id,
        display_name: 'L17 fixture model',
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
      if (!enabled.ok) return { error: JSON.stringify(enabled) };
      const current = await api.getModelSettings();
      if (!current.ok) return { error: 'model settings' };
      const chosen = await api.setDefaultModel({
        expected_revision: current.value.aura_revision,
        model_profile_id: profile.value.value.id,
      });
      if (!chosen.ok) return { error: JSON.stringify(chosen) };
      const defaults = await api.getPermissionDefaults();
      if (!defaults.ok) return { error: JSON.stringify(defaults) };
      if (defaults.value.mode !== mode) {
        const set = await api.setPermissionDefaults({
          expected_revision: defaults.value.revision,
          mode,
        });
        if (!set.ok) return { error: JSON.stringify(set) };
      }
      return { profile: profile.value.value.id };
    },
    { url, mode },
  );
  expect(result.error, JSON.stringify(result)).toBeUndefined();
  return result.profile as string;
}

/** A workspace at `root`, added as the user adds one. */
export async function workspace(page: Page, root: string, name: string): Promise<string> {
  const id = await page.evaluate(
    async ({ root, name }) => {
      const created = await window.aura.v1.createWorkspace({
        display_name: name,
        selected_root: root,
      });
      return created.ok ? created.value.workspace.workspace_id : JSON.stringify(created);
    },
    { root, name },
  );
  expect(id).toMatch(/^wsp_/u);
  return id;
}

/** Settings → 插件, as the user opens it. */
export async function pluginSettings(page: Page) {
  const navigation = page.getByRole('navigation', { name: '设置分类' });
  if (!(await navigation.isVisible()))
    await page.getByRole('button', { name: '设置', exact: true }).click();
  await navigation.getByRole('button', { name: '插件', exact: true }).click();
  return page.getByRole('region', { name: '插件设置' });
}

/** The plugin's row on Settings → 插件; the list's own items only. */
export function pluginRow(plugins: Locator, name: string): Locator {
  return plugins
    .getByRole('list', { name: '插件' })
    .locator(':scope > li')
    .filter({ hasText: name });
}

/** The plugin's dialog, opened from its row on Settings → 插件. */
export async function pluginDialog(page: Page, row: Locator, name: string): Promise<Locator> {
  await row.getByRole('button', { name: `配置 ${name}` }).click();
  return page.getByRole('dialog', { name });
}

/**
 * The folder added as a plugin in development through Settings → 插件 (Main's folder dialog
 * answers with it), then its contents reviewed and trusted in its dialog, which is closed again.
 */
export async function addAndTrust(
  app: ElectronApplication,
  page: Page,
  folder: string,
  name: string,
) {
  const plugins = await pluginSettings(page);
  await app.evaluate(({ dialog }, path) => {
    Reflect.set(dialog, 'showOpenDialog', async () => ({ canceled: false, filePaths: [path] }));
  }, folder);
  await plugins.getByRole('button', { name: '添加开发中的插件…' }).click();
  const row = pluginRow(plugins, name);
  await expect(row).toContainText('开发中');
  await row.getByRole('button', { name: '查看并信任' }).click();
  const dialog = page.getByRole('dialog', { name });
  await dialog
    .getByRole('region', { name: `${name} 的内容` })
    .getByRole('button', { name: '信任并启用' })
    .click();
  await expect(row).toContainText('可用');
  await dialog.getByRole('button', { name: '关闭' }).click();
  return row;
}

/** The plugin's settings that only Settings → 插件 changes, saved through its own route. */
export async function configure(page: Page, name: string, change: Record<string, unknown>) {
  const result = await page.evaluate(
    async ({ name, change }) => {
      const api = window.aura.v1;
      const listed = await api.listPlugins({});
      if (!listed.ok) return listed;
      return api.configurePlugin({
        name,
        expected_revision: listed.value.revision,
        ...change,
      } as Parameters<typeof api.configurePlugin>[0]);
    },
    { name, change },
  );
  expect(result.ok, JSON.stringify(result)).toBe(true);
}

/** The text a plugin's panel page shows now, read from Main where the page lives. */
export async function panelText(app: ElectronApplication, plugin: string): Promise<string> {
  return app.evaluate(async ({ webContents }, prefix) => {
    const page = webContents.getAllWebContents().find((each) => each.getURL().startsWith(prefix));
    if (page === undefined) return '';
    return String(await page.executeJavaScript('document.body.innerText'));
  }, `aura-plugin://${plugin}/`);
}
