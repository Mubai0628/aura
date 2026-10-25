import { expect, test, type Page } from '@playwright/test';
import { createServer as createNetServer, connect as connectNet, type Socket } from 'node:net';
import { createServer as createTlsServer } from 'node:tls';
import { mkdir, mkdtemp, readdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { armReadOnlyWrite } from './s06/setup';
import { finished, launch, pending, submit } from './s06/harness';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from './s06-file-write-provider.mjs';

const CERTIFICATE = resolve(import.meta.dirname, '../../../fixtures/network/l02/certificate.pem');
// The public synthetic key the Rust proxy fixture uses too. It is not a real credential.
const KEY = `-----BEGIN PRIVATE KEY-----
MIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQgWnMihkcZABZdwVb3
6z7FPjDKVr5urHHr0F0i2+c6WmqhRANCAATghvDuEXFevIkWyS8vQDeTQF7VZBFC
/9J/SyLQ9KgQZIMV6E/sY5Fh/5a+rF4iiFiXDZnGPbHYvT+C/hUzg7RK
-----END PRIVATE KEY-----
`;

// Each payload the desktop rejects is logged and resets the Core connection. Once the Tool
// settings projection carried the saved proxy, and the desktop reconnected in a loop until it
// gave up and showed that Core could not start.
async function protocolViolations(root: string): Promise<number> {
  const namespaces = join(root, 'Local', 'Aura', 'namespaces');
  let count = 0;
  for (const namespace of await readdir(namespaces)) {
    const log = await readFile(join(namespaces, namespace, 'logs', 'main.jsonl'), 'utf8').catch(
      () => '',
    );
    count += log.split('"core_protocol_violation"').length - 1;
  }
  return count;
}

function track(server: { on(event: 'connection', listener: (socket: Socket) => void): void }) {
  const sockets = new Set<Socket>();
  server.on('connection', (socket) => {
    sockets.add(socket);
    socket.on('close', () => sockets.delete(socket));
  });
  return sockets;
}

function stop(
  server: { close(callback: (error?: Error) => void): void },
  sockets: Set<Socket>,
): Promise<void> {
  for (const socket of sockets) socket.destroy();
  return new Promise((done, reject) => {
    server.close((error) => (error ? reject(error) : done()));
  });
}

function listen(server: {
  once(event: 'error', listener: (error: Error) => void): void;
  listen(port: number, host: string, listening: () => void): void;
  address(): string | { port: number } | null;
}): Promise<number> {
  return new Promise((done, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      if (address === null || typeof address === 'string') {
        reject(new Error('fixture did not bind a TCP port'));
        return;
      }
      done(address.port);
    });
  });
}

async function httpsTarget() {
  const cert = await readFile(CERTIFICATE);
  const server = createTlsServer({ cert, key: KEY }, (socket) => {
    socket.once('data', () => {
      const body =
        '<html><body>proxied-ok<ul><li class="b_algo"><h2><a href="https://example.com/aura">Aura fixture result</a></h2><p>Local proxy search fixture</p></li></ul></body></html>';
      socket.end(
        `HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: ${Buffer.byteLength(body)}\r\nConnection: close\r\n\r\n${body}`,
      );
    });
  });
  const sockets = track(server);
  const port = await listen(server);
  return {
    server,
    sockets,
    port,
    url: `https://proxy-target.test:${port}/resource`,
  };
}

async function connectProxy(targetPort: number) {
  const authorities: string[] = [];
  const server = createNetServer((socket) => {
    const chunks: Buffer[] = [];
    const onData = (chunk: Buffer) => {
      chunks.push(chunk);
      const buffer = Buffer.concat(chunks);
      const headerEnd = buffer.indexOf('\r\n\r\n');
      if (headerEnd < 0) return;
      socket.off('data', onData);
      const line = buffer.subarray(0, headerEnd).toString('latin1').split('\r\n')[0] ?? '';
      const match = /^CONNECT (\S+) HTTP\/1\.[01]$/.exec(line);
      authorities.push(match?.[1] ?? line);
      const rest = buffer.subarray(headerEnd + 4);
      const upstream = connectNet(targetPort, '127.0.0.1', () => {
        socket.write('HTTP/1.1 200 Connection Established\r\n\r\n');
        if (rest.length > 0) upstream.write(rest);
        socket.pipe(upstream);
        upstream.pipe(socket);
      });
      upstream.on('error', () => socket.destroy());
    };
    socket.on('data', onData);
    socket.on('error', () => undefined);
  });
  const sockets = track(server);
  const port = await listen(server);
  return { server, sockets, port, authorities };
}

function gatedProvider() {
  let callNetwork = false;
  const phases = new Map<string, 'searched' | 'loaded' | 'called'>();
  return writeProvider({
    path: 'unused.txt',
    text: '',
    planAction(request, action): ProviderAction {
      if (!callNetwork) return { tool: null, arguments: {} };
      const phase = phases.get(request.turn);
      if (phase === 'called') return { tool: null, arguments: {} };
      if (phase === 'searched') {
        const page = lastTool(request) as {
          results: Array<{ canonical_name: string; tool_id: string }>;
        };
        const hit = page.results.find((result) => result.canonical_name === action.tool);
        if (!hit) throw new Error(`the real registry must contain ${action.tool}`);
        phases.set(request.turn, 'loaded');
        return { tool: 'tools.load', arguments: { tool_ids: [hit.tool_id], replace: [] } };
      }
      const prefix = `${String(action.tool).replaceAll('.', '_')}__`;
      const visible = request.tools.some((tool) =>
        (tool as { function: { name: string } }).function.name.startsWith(prefix),
      );
      if (!visible) {
        phases.set(request.turn, 'searched');
        return { tool: 'tools.search', arguments: { query: action.tool, limit: 50, cursor: null } };
      }
      phases.set(request.turn, 'called');
      return action;
    },
  }).then((provider) => ({
    ...provider,
    arm(tool: string, args: Record<string, unknown>) {
      callNetwork = true;
      provider.setAction(tool, args);
    },
  }));
}

function lastTool(request: ProviderRequest): Record<string, unknown> {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  if (!message) throw new Error('Core did not return the tool result');
  const result = JSON.parse(message.content) as {
    outcome: { type: string; payload: { value: Record<string, unknown> } };
  };
  if (result.outcome.type !== 'succeeded') throw new Error(message.content);
  return result.outcome.payload.value;
}

async function activity(page: Page, session: string) {
  return page.evaluate(async (session_id) => {
    const result = await window.aura.v1.readAgentToolActivity({
      session_id,
      before_call_id: null,
      limit: 20,
    });
    if (!result.ok) throw new Error(JSON.stringify(result.error));
    return result.value.calls.map((call) => ({
      title: call.display.title,
      details: call.display.details,
    }));
  }, session);
}

test('L01 sets a loopback proxy, reaches HTTPS through it, and explains a dead proxy', async () => {
  const info = test.info();
  test.setTimeout(300_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-l01-proxy-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const target = await httpsTarget();
  const proxy = await connectProxy(target.port);
  const certificate = await readFile(CERTIFICATE, 'utf8');
  const fixture = join(root, 'fixture.json');
  await writeFile(
    fixture,
    JSON.stringify({
      certificate_pem: certificate,
      records: [
        {
          host: 'proxy-target.test',
          classified: ['8.8.8.8'],
          connect: ['127.0.0.1'],
        },
        {
          host: 'cn.bing.com',
          classified: ['8.8.8.8'],
          connect: ['127.0.0.1'],
        },
      ],
    }),
  );
  const provider = await gatedProvider();
  const { app, page } = await launch(root, { AURA_L01_NETWORK_FIXTURE: fixture });
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'L01 network proxy',
      text: '准备工具设置',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    await finished(page, session);
    await page.getByRole('button', { name: '设置', exact: true }).click();
    await page
      .locator('.settings-navigation')
      .getByRole('button', { name: '工具', exact: true })
      .click();
    const section = page.getByRole('region', { name: '网络代理' });
    // The section carries no explanation.
    await expect(section.getByRole('switch', { name: '启用网络代理' })).toBeVisible();
    await expect(section).not.toContainText('只用于 http');
    // A proxy filled in and saved while it stays disabled. Reopening
    // Tool settings then receives that saved proxy again.
    await section.getByLabel('代理协议').selectOption('socks5');
    await section.getByLabel('代理地址').fill('127.0.0.1');
    await section.getByLabel('代理端口').fill(String(proxy.port));
    await section.getByRole('button', { name: '保存网络代理', exact: true }).click();
    await expect(section.getByText('网络代理已保存。')).toBeVisible();
    const navigation = page.locator('.settings-navigation');
    await navigation.getByRole('button', { name: '模型', exact: true }).click();
    await navigation.getByRole('button', { name: '工具', exact: true }).click();
    await expect(page.getByLabel('默认 Timeout（毫秒）')).toHaveValue('600000');
    await expect(section.getByLabel('代理端口')).toHaveValue(String(proxy.port));
    await page.waitForTimeout(1_000);
    expect(await protocolViolations(root)).toBe(0);
    await section.getByLabel('启用网络代理').check();
    await section.getByLabel('代理协议').selectOption('http');
    await section.getByLabel('代理地址').fill('127.0.0.1');
    await section.getByLabel('代理端口').fill(String(proxy.port));
    await section.getByRole('button', { name: '测试连接', exact: true }).click();
    await expect(section.getByRole('status').filter({ hasText: '连接成功' })).toBeVisible();
    await section.getByRole('button', { name: '保存网络代理', exact: true }).click();
    await expect(section.getByText('网络代理已保存。')).toBeVisible();
    await section.getByLabel('代理端口').fill(String(target.port));
    await section.getByRole('button', { name: '保存网络代理', exact: true }).click();
    await expect(section.getByText('网络代理已保存。')).toBeVisible();
    await section.getByRole('button', { name: '恢复上一版有效代理', exact: true }).click();
    const preview = section.getByRole('region', { name: '代理恢复预览' });
    await expect(preview).toContainText(`127.0.0.1:${proxy.port}`);
    await preview.getByRole('button', { name: '应用代理恢复', exact: true }).click();
    // The command receipt can precede the committed projection. Wait for that
    // authority rather than a short-lived notification under a busy machine.
    await expect
      .poll(
        async () =>
          page.evaluate(async () => {
            const result = await window.aura.v1.refreshToolProjection({
              projection_kind: 'tool_settings',
              projection_scope: { type: 'global' },
            });
            if (!result.ok || result.value.projection_kind !== 'tool_settings') return null;
            return result.value.payload.settings.network_proxy;
          }),
        { timeout: 30_000 },
      )
      .toEqual({ enabled: true, protocol: 'http', host: '127.0.0.1', port: proxy.port });
    await expect(section.getByLabel('代理端口')).toHaveValue(String(proxy.port));
    await page.getByRole('button', { name: '返回会话', exact: true }).click();
    provider.arm('http.request', { url: target.url });
    await submit(page, session, '经本机代理读取测试 HTTPS');
    await pending(page, session);
    // The card shows the model's explanation; the facts are under 查看详情.
    await page.locator('section[aria-label="当前任务审批"]').getByText('查看详情').click();
    await expect(
      page.getByText(`访问公网地址时将经本机代理 127.0.0.1:${proxy.port}（HTTP）发出`),
    ).toBeVisible();
    await page
      .locator('section[aria-label="当前任务审批"]')
      .getByRole('button', { name: '本会话允许', exact: true })
      .click();
    await finished(page, session);
    expect(proxy.authorities).toContain(`proxy-target.test:${target.port}`);
    expect(JSON.stringify(provider.requests)).toContain('proxied-ok');
    expect(JSON.stringify(provider.requests)).not.toContain('user_proxy');
    const succeeded = await activity(page, session);
    expect(
      succeeded.some((call) => call.title === 'http.request' && call.details.includes('经代理')),
    ).toBe(true);
    provider.arm('web.search', { query: 'aura fixture', max_results: 1 });
    await submit(page, session, '经本机代理搜索本地 TLS 夹具');
    await finished(page, session);
    expect(proxy.authorities).toContain('cn.bing.com:443');
    expect(JSON.stringify(provider.requests)).toContain('Aura fixture result');
    expect(JSON.stringify(provider.requests)).toContain('https://example.com/aura');
    provider.arm('http.request', { url: target.url });
    await stop(proxy.server, proxy.sockets);
    await stop(target.server, target.sockets);
    await submit(page, session, '代理已关闭后再请求一次');
    await finished(page, session);
    const transcript = JSON.stringify(provider.requests);
    expect(transcript).toContain('尚未向目标发出 HTTP 请求');
    const failed = await activity(page, session);
    const refused = failed.find(
      (call) =>
        call.title === 'http.request' &&
        call.details.join('\n').includes('尚未向目标发出 HTTP 请求'),
    );
    expect(refused).toBeTruthy();
    expect(refused?.details.includes('经代理')).toBe(false);
    expect(provider.errors).toEqual([]);
    await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  } finally {
    for (const sockets of [proxy.sockets, target.sockets]) {
      for (const socket of sockets) socket.destroy();
    }
    proxy.server.close(() => undefined);
    target.server.close(() => undefined);
    await app.close();
  }
});
