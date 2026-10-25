// Diagnosis kept as the spec-faithful reproduction:
//   default         : replicate the spec's interaction order, then open Settings
//   AURA_P05_BARE=1 : open Settings directly, without the prior interactions
// The bare run is the control showing that the failure depended on the preceding
// scoped-projection lifecycle rather than on the settings page itself.
import { join } from 'node:path';

import { runDiagnostic, launch, writeJson } from './harness.mjs';

const bare = process.env.AURA_P05_BARE === '1';

await runDiagnostic(bare ? 'p05-bare' : 'p05-sequence', async ({ dir, log }) => {
  const application = await launch({ dataRoot: join(dir, 'data') });
  const window = await application.firstWindow();
  await window.getByText('Aura Core 已就绪').waitFor({ state: 'visible', timeout: 20000 });
  log('core ready');

  const step = async (label, run) => {
    const started = Date.now();
    await run();
    log(`step ${label}: ok ${Date.now() - started}ms`);
  };

  if (!bare) {
    await step('hover-title', () =>
      window.getByRole('heading', { name: '实现模型配置与 Transport' }).hover(),
    );
    await step('click-title', () =>
      window.getByRole('heading', { name: '实现模型配置与 Transport' }).click(),
    );
    await step('open-model-menu', () =>
      window.getByRole('button', { name: /模型未选择/u }).click(),
    );
    await step('menu-visible', () =>
      window
        .getByRole('menu', { name: '快速选择模型' })
        .waitFor({ state: 'visible', timeout: 5000 }),
    );
    await step('close-menu', () =>
      window.getByRole('heading', { name: '实现模型配置与 Transport' }).click(),
    );
    await step('menu-hidden', () =>
      window
        .getByRole('menu', { name: '快速选择模型' })
        .waitFor({ state: 'hidden', timeout: 5000 }),
    );
    await step('open-files', () => window.getByRole('button', { name: /^文件/u }).click());
    await step('files-visible', () =>
      window.getByRole('searchbox', { name: '筛选文件' }).waitFor({ timeout: 5000 }),
    );
  }

  await step('open-settings', () => window.getByRole('button', { name: '设置' }).click());
  await step('settings-heading', () =>
    window.getByRole('heading', { level: 1, name: '设置' }).waitFor({ timeout: 5000 }),
  );

  const heading = window.getByRole('heading', { level: 2, name: '模型配置' });
  let headingVisible = false;
  try {
    await heading.waitFor({ state: 'visible', timeout: 12000 });
    headingVisible = true;
    log('MODEL HEADING VISIBLE');
  } catch {
    log('MODEL HEADING MISSING');
    log('status text:', JSON.stringify(await window.getByRole('status').allTextContents()));
  }
  const probe = await window.evaluate(async () => {
    const started = Date.now();
    const settings = await globalThis.aura.v1.getModelSettings();
    return { ms: Date.now() - started, ok: settings?.ok, code: settings?.error?.code };
  });
  log('settings probe:', JSON.stringify(probe));
  writeJson(dir, 'result.json', { bare, headingVisible, probe });
  if (!headingVisible) {
    throw new Error('the model configuration heading never appeared');
  }
  return application;
});
