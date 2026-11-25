import type { Session } from 'electron';
import {
  DEVELOPMENT_RENDERER_HEADER,
  DEVELOPMENT_RENDERER_URL,
  isDevelopmentRendererInstance,
} from '../shared/development-renderer.js';

interface DevelopmentRenderer {
  url: string;
  instance: string;
}

export function developmentRenderer(
  env: NodeJS.ProcessEnv,
  packaged: boolean,
): DevelopmentRenderer | undefined {
  if (packaged || env.AURA_RENDERER_URL === undefined) return;
  const instance = env.AURA_DEV_RENDERER_INSTANCE;
  if (
    env.AURA_RENDERER_URL !== DEVELOPMENT_RENDERER_URL ||
    !isDevelopmentRendererInstance(instance)
  ) {
    throw new Error('开发界面地址或启动标记无效，请通过 corepack pnpm dev 启动 Aura');
  }
  return { url: DEVELOPMENT_RENDERER_URL, instance };
}

export function guardDevelopmentRenderer(
  session: Session,
  renderer: DevelopmentRenderer | undefined,
): void {
  if (renderer === undefined) return;
  session.webRequest.onHeadersReceived({ urls: [`${renderer.url}/*`] }, (details, answer) => {
    const instances = Object.entries(details.responseHeaders ?? {})
      .filter(([name]) => name.toLowerCase() === DEVELOPMENT_RENDERER_HEADER)
      .flatMap(([, value]) => value);
    answer({ cancel: instances.length !== 1 || instances[0] !== renderer.instance });
  });
}
