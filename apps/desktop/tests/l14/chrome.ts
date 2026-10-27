import { chromium, type BrowserContext, type Worker } from '@playwright/test';
import { execFile } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { createServer, type Server } from 'node:http';
import { join, resolve } from 'node:path';
import { promisify } from 'node:util';

const run = promisify(execFile);
const repository = resolve(import.meta.dirname, '../../../..');
const HOSTS = 'HKCU\\Software\\Google\\Chrome\\NativeMessagingHosts';
/** The extension id of the test key; a development namespace pairs only with it. */
export const TEST_EXTENSION_ID = 'bheiijocpacefhgapeegbnekchcpedkj';

const sha256 = (text: string) => createHash('sha256').update(text, 'utf8').digest('hex');

/** The namespace this checkout's development runs, and so these tests, run in. */
export async function developmentNamespace(): Promise<string> {
  const manifest = JSON.parse(
    await readFile(join(repository, 'target/aura-dev-runtime/component-manifest.json'), 'utf8'),
  ) as { bundle_namespace: string };
  return manifest.bundle_namespace;
}

/** As aura-chrome-domain's native_host_name for a namespace that is not the released one. */
export const hostName = (namespace: string) =>
  `com.aura.chrome_control.t${sha256(namespace).slice(0, 12)}`;

/** The namespace's prefix in the Credential Manager. */
export const credentialPrefix = (namespace: string) => `Aura/${sha256(namespace).slice(0, 32)}/`;

/** Builds the test extension for `namespace` into `out`. */
export async function buildTestExtension(namespace: string, out: string): Promise<void> {
  const folder = join(repository, 'apps/chrome-extension');
  await run(process.execPath, [join(folder, 'node_modules/vite/bin/vite.js'), 'build'], {
    cwd: folder,
    env: {
      ...process.env,
      AURA_CHROME_TARGET: 'test',
      AURA_CHROME_NAMESPACE: namespace,
      AURA_CHROME_OUT_DIR: out,
    },
  });
}

/** The manifest Chrome finds for the host, or null when it is not registered. */
export async function registeredManifest(name: string): Promise<string | null> {
  try {
    const { stdout } = await run('reg', ['query', `${HOSTS}\\${name}`, '/ve']);
    return /REG_SZ\s+(.+?)\s*$/mu.exec(stdout)?.[1] ?? null;
  } catch {
    return null;
  }
}

export async function removeRegistration(name: string): Promise<void> {
  await run('reg', ['delete', `${HOSTS}\\${name}`, '/f']).catch(() => undefined);
}

/** Credential Manager targets under `prefix`. */
export async function credentials(prefix: string): Promise<string[]> {
  const { stdout } = await run('cmdkey', [`/list:${prefix}*`]);
  return [...stdout.matchAll(/Target: (?:LegacyGeneric:target=)?(\S+)/gu)].map(
    (match) => match[1]!,
  );
}

export async function deleteCredential(target: string): Promise<void> {
  await run('cmdkey', [`/delete:${target}`]).catch(() => undefined);
}

/** The images of running relay processes. */
export async function runningRelays(): Promise<string[]> {
  const { stdout } = await run('powershell', [
    '-NoProfile',
    '-Command',
    // Get-Process fails when there is none; that is an empty list here.
    'Get-Process -Name aura-chrome-host -ErrorAction SilentlyContinue | ForEach-Object { $_.Path }; exit 0',
  ]);
  return stdout
    .split(/\r?\n/u)
    .map((line) => line.trim())
    .filter(Boolean);
}

/** Chrome for Testing with only the given extension loaded, in a profile of its own. */
export async function launchChrome(
  profile: string,
  extension: string,
): Promise<{ context: BrowserContext; worker: Worker; extensionId: string }> {
  const context = await chromium.launchPersistentContext(profile, {
    channel: 'chromium',
    headless: process.env.AURA_E2E_CHROME_HEADED !== '1',
    args: [`--disable-extensions-except=${extension}`, `--load-extension=${extension}`],
  });
  const worker = context.serviceWorkers()[0] ?? (await context.waitForEvent('serviceworker'));
  return { context, worker, extensionId: new URL(worker.url()).host };
}

const PAGES: Record<string, string> = {
  '/a.html': `<!doctype html><meta charset="utf-8"><title>甲页面</title><h1>甲页面</h1>
<label>名字 <input id="name" oninput="fetch('/hit?what=typed:'+encodeURIComponent(this.value))"></label>
<button onclick="count.textContent=String(Number(count.textContent)+1);fetch('/hit?what=added')">加一</button>
<p>次数 <span id="count">0</span></p>`,
  '/b.html': `<!doctype html><meta charset="utf-8"><title>乙页面</title><h1>乙页面</h1>
<button onclick="state.textContent='已删除';fetch('/hit?what=erased')">删除全部记录</button>
<p id="state">未删除</p>`,
};

/** Two sites on loopback, each its own origin; `hits` records what their pages report. */
export async function startSites(): Promise<{
  a: string;
  b: string;
  hits: string[];
  close: () => Promise<void>;
}> {
  const hits: string[] = [];
  const serve = async (): Promise<{ server: Server; port: number }> => {
    const server = createServer((request, response) => {
      const url = new URL(request.url ?? '/', 'http://127.0.0.1');
      if (url.pathname === '/hit') {
        hits.push(url.searchParams.get('what') ?? '');
        response.end('ok');
        return;
      }
      const page = PAGES[url.pathname];
      response.writeHead(page === undefined ? 404 : 200, {
        'content-type': 'text/html; charset=utf-8',
      });
      response.end(page ?? 'not found');
    });
    await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
    const address = server.address();
    if (address === null || typeof address === 'string') throw new Error('no site address');
    return { server, port: address.port };
  };
  const first = await serve();
  const second = await serve();
  return {
    a: `http://127.0.0.1:${first.port}/a.html`,
    b: `http://127.0.0.1:${second.port}/b.html`,
    hits,
    close: async () => {
      for (const { server } of [first, second])
        await new Promise<void>((done) => server.close(() => done()));
    },
  };
}
