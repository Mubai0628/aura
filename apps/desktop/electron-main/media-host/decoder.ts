import { randomUUID } from 'node:crypto';
import { BrowserWindow, session, type Session } from 'electron';
import { mediaPageRun } from './decoder-page.js';
import { checkedMediaResult } from './results.js';
import type { MediaSource } from './source.js';
import { MediaFailure, MediaRefusal, type MediaCommand, type MediaDecoded } from './types.js';

const CSP =
  "default-src 'none'; media-src 'self'; script-src 'none'; connect-src 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";
let privateSession: Session | undefined;
let active: { window: BrowserWindow; source: MediaSource; page: string } | undefined;

function sandboxSession(): Session {
  if (privateSession) return privateSession;
  const isolated = session.fromPartition(`aura-media-decoder-${randomUUID()}`, { cache: false });
  isolated.setPermissionRequestHandler((_contents, _permission, done) => done(false));
  isolated.setPermissionCheckHandler(() => false);
  isolated.on('will-download', (event) => event.preventDefault());
  isolated.webRequest.onBeforeRequest((details, done) => {
    const job = active;
    done({
      cancel:
        !job ||
        details.webContentsId !== job.window.webContents.id ||
        ![job.page, job.source.url].includes(details.url),
    });
  });
  isolated.protocol.handle('aura', async (request) => {
    const job = active;
    if (!job) return new Response('拒绝：媒体命令已结束。', { status: 403 });
    if (request.url === job.page && request.method === 'GET')
      return new Response(
        '<!doctype html><meta charset="utf-8"><title>Aura media decoder</title><body></body>',
        {
          headers: {
            'Content-Type': 'text/html; charset=utf-8',
            'Content-Security-Policy': CSP,
            'Cache-Control': 'no-store',
            'X-Content-Type-Options': 'nosniff',
          },
        },
      );
    return job.source.serve(request);
  });
  privateSession = isolated;
  return isolated;
}

/** A hidden sandbox with no path, renderer bridge, Node or external network capability. */
export async function decodeMedia(
  command: MediaCommand,
  source: MediaSource,
  signal: AbortSignal,
): Promise<MediaDecoded> {
  if (signal.aborted) throw new MediaFailure('媒体请求已停止。');
  if (active) throw new MediaRefusal('媒体解码正在忙，请稍后重试。');
  const window = new BrowserWindow({
    show: false,
    width: 640,
    height: 480,
    webPreferences: {
      session: sandboxSession(),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      nodeIntegrationInSubFrames: false,
      nodeIntegrationInWorker: false,
      webviewTag: false,
      webSecurity: true,
      allowRunningInsecureContent: false,
      backgroundThrottling: false,
      offscreen: true,
      autoplayPolicy: 'no-user-gesture-required',
    },
  });
  const page = source.url.replace(/\/source$/u, '/index.html');
  active = { window, source, page };
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', (event) => event.preventDefault());
  window.webContents.on('will-redirect', (event) => event.preventDefault());
  window.webContents.on('will-attach-webview', (event) => event.preventDefault());
  const close = () => {
    source.close();
    if (!window.isDestroyed()) window.destroy();
  };
  const timer = setTimeout(close, command.timeout_ms);
  signal.addEventListener('abort', close, { once: true });
  try {
    const container = await source.container();
    await window.loadURL(page);
    const answer: Awaited<ReturnType<typeof mediaPageRun>> =
      await window.webContents.executeJavaScript(
        `(${mediaPageRun.toString()})(${JSON.stringify({ url: source.url, size: command.file_size, action: command.action, container, timeout: command.timeout_ms })})`,
      );
    if (!answer || answer.ok !== true) {
      if (answer?.refused) throw new MediaRefusal(answer.message);
      throw new MediaFailure(answer?.message ?? '媒体解码没有返回结果。');
    }
    return checkedMediaResult(answer.value, command);
  } catch (error) {
    if (error instanceof MediaRefusal || error instanceof MediaFailure) throw error;
    if (signal.aborted) throw new MediaFailure('媒体请求已停止。');
    throw new MediaFailure('媒体解码失败或超时，Chromium无法读取这个视频。');
  } finally {
    clearTimeout(timer);
    signal.removeEventListener('abort', close);
    close();
    if (active?.window === window) active = undefined;
    await privateSession?.clearCache().catch(() => undefined);
  }
}
