import { request } from 'node:http';
import { createConnection } from 'node:net';
import { setTimeout as delay } from 'node:timers/promises';
import {
  DEVELOPMENT_RENDERER_HEADER,
  DEVELOPMENT_RENDERER_URL,
  isDevelopmentRendererInstance,
} from '../../apps/desktop/shared/development-renderer.ts';

export function rendererPortInUse(origin = DEVELOPMENT_RENDERER_URL) {
  const { hostname, port } = new URL(origin);
  return new Promise((resolve, reject) => {
    const socket = createConnection({ host: hostname, port: Number(port) });
    socket.once('connect', () => {
      socket.destroy();
      resolve(true);
    });
    socket.once('error', (error) => {
      if (error.code === 'ECONNREFUSED') resolve(false);
      else reject(error);
    });
    socket.setTimeout(1_000, () => socket.destroy(new Error('查询开发界面端口超时')));
  });
}

export async function waitForDevelopmentRenderer(
  instance,
  { origin = DEVELOPMENT_RENDERER_URL, timeoutMs = 30_000 } = {},
) {
  if (!isDevelopmentRendererInstance(instance)) throw new Error('缺少本次开发界面的启动标记');
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await matchesRenderer(origin, instance)) return;
    await delay(75);
  }
  throw new Error('Aura 的开发界面未就绪。请查看开发界面的启动日志。');
}

function matchesRenderer(origin, instance) {
  return new Promise((resolve) => {
    const probe = request(origin, { method: 'HEAD', agent: false, timeout: 1_000 }, (response) => {
      response.resume();
      resolve(
        response.statusCode === 200 && response.headers[DEVELOPMENT_RENDERER_HEADER] === instance,
      );
    });
    probe.once('error', () => resolve(false));
    probe.once('timeout', () => probe.destroy());
    probe.end();
  });
}
