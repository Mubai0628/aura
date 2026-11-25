import { spawnSync } from 'node:child_process';
import { DEVELOPMENT_RENDERER_URL } from '../../apps/desktop/shared/development-renderer.ts';
import { waitForDevelopmentRenderer } from './renderer.mjs';

if (process.env.AURA_RENDERER_URL === undefined) {
  const pnpm = process.env.npm_execpath;
  if (!pnpm) throw new Error('请通过 corepack pnpm dev 启动 Aura');
  const build = spawnSync(process.execPath, [pnpm, 'build:renderer'], {
    stdio: 'inherit',
    windowsHide: true,
  });
  if (build.error) throw build.error;
  process.exitCode = build.status ?? 1;
} else {
  if (process.env.AURA_RENDERER_URL !== DEVELOPMENT_RENDERER_URL)
    throw new Error(`Aura 的开发界面地址必须为 ${DEVELOPMENT_RENDERER_URL}`);
  await waitForDevelopmentRenderer(process.env.AURA_DEV_RENDERER_INSTANCE);
}
