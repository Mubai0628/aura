import { _electron as electron, expect, test, type ElectronApplication } from '@playwright/test';
import { execFileSync } from 'node:child_process';
import { createHash, randomBytes } from 'node:crypto';
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { basename, join, resolve, sep } from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { addAndTrust, configure, pluginDialog, pluginSettings, pluginRow } from './l17/setup';

// A random synthetic credential only; recording is disabled even on failure.
test.use({ trace: 'off', screenshot: 'off', video: 'off' });
const PROBE = resolve(import.meta.dirname, 'l17/credential-probe.ps1');
const IDLE = resolve(import.meta.dirname, '../../../scripts/l18/desktop-idle.ps1');

function credentials(owner: string, targets: string[], action: 'inspect' | 'cleanup'): number {
  return targets.reduce((count, target) => {
    const result = JSON.parse(
      execFileSync(
        'pwsh.exe',
        ['-NoProfile', '-File', PROBE, '-Owner', owner, '-Target', target, '-Action', action],
        { encoding: 'utf8', windowsHide: true },
      ),
    ) as { exists: boolean };
    return count + Number(result.exists);
  }, 0);
}

async function registeredTargets(root: string, owner: string, seen: Set<string>) {
  const entries = await readdir(root, { recursive: true });
  const databases = entries.filter((path) => path.endsWith('aura-v2.db'));
  if (databases.length > 1) throw new Error('Multiple isolated journals found');
  if (databases.length === 1) {
    const db = new DatabaseSync(join(root, databases[0]!), { readOnly: true });
    try {
      for (const row of db
        .prepare('SELECT secret_id FROM mcp_secrets WHERE server_id=?')
        .all(owner)) {
        const id = String(row.secret_id);
        if (!/^[A-Za-z0-9_:-]{1,128}$/u.test(id)) throw new Error('Invalid test credential ID');
        seen.add(`Aura/mcp/secret/${id}`);
      }
    } finally {
      db.close();
    }
  }
  return [...seen];
}

async function ownerIdle() {
  const state = JSON.parse(
    execFileSync('pwsh.exe', ['-NoProfile', '-File', IDLE], {
      encoding: 'utf8',
      windowsHide: true,
    }),
  ) as { idleMs: number; lastInputTick: number; unlocked: boolean };
  let eligible = state.unlocked && state.idleMs >= 600_000;
  if (!eligible && state.unlocked && process.env.AURA_L18_OWN_INPUT_RECORD) {
    const record = JSON.parse(await readFile(process.env.AURA_L18_OWN_INPUT_RECORD, 'utf8')) as {
      ownTicks: number[];
      ownerLastUtc: string;
    };
    eligible =
      record.ownTicks.includes(state.lastInputTick) &&
      Date.now() - Date.parse(record.ownerLastUtc) >= 600_000;
  }
  console.log(JSON.stringify({ idleMs: state.idleMs, unlocked: state.unlocked, eligible }));
  return eligible;
}

const SERVICE = `import { createInterface } from 'node:readline';
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
const lines = createInterface({ input: process.stdin });
lines.on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    const value = process.env.AURA_SECRET_TOKEN || '';
    writeFileSync(join(process.env.AURA_PLUGIN_DATA, 'received.digest'), createHash('sha256').update(value).digest('hex'));
    process.stdout.write(JSON.stringify({ type: 'ready', protocol: 1, channels: [] }) + '\\n');
    process.stderr.write('fixture-secret=' + value + '\\n');
  }
  if (message.type === 'shutdown') process.exit(0);
});
lines.on('close', () => process.exit(0));
`;

async function plugin(root: string, name: string) {
  const folder = join(root, 'source', name);
  await mkdir(join(folder, 'service'), { recursive: true });
  await writeFile(
    join(folder, 'aura-plugin.toml'),
    `schema_version = 2
name = "${name}"
version = "0.1.0"
description = "Temporary credential acceptance fixture"
capabilities = []
[service]
command = ${JSON.stringify(process.execPath.replaceAll('\\', '/'))}
arguments = ["\${PLUGIN_ROOT}/service/main.mjs"]
[[settings]]
key = "token"
title = "Token"
kind = "secret"
required = true
`,
  );
  await writeFile(join(folder, 'service/main.mjs'), SERVICE);
  return folder;
}

async function files(root: string): Promise<string[]> {
  const entries = await readdir(root, { withFileTypes: true });
  return (
    await Promise.all(
      entries.map(async (entry) =>
        entry.isDirectory() ? files(join(root, entry.name)) : [join(root, entry.name)],
      ),
    )
  ).flat();
}

async function cleanup(
  app: ElectronApplication | undefined,
  owner: string,
  root: string,
  seen: Set<string>,
) {
  const errors: Error[] = [];
  let credentialCleanupVerified = false;
  let testRootRemoved = false;
  try {
    await app?.close();
  } catch {
    errors.push(new Error('Isolated Aura could not be closed'));
  }
  try {
    credentialCleanupVerified =
      credentials(owner, await registeredTargets(root, owner, seen), 'cleanup') === 0;
    if (!credentialCleanupVerified) errors.push(new Error('Test credentials remain'));
  } catch {
    errors.push(new Error('Test credential cleanup could not be verified'));
  }
  try {
    if (!credentialCleanupVerified || errors.length)
      throw new Error('Retain isolated metadata until cleanup is verified');
    const absolute = resolve(root);
    if (
      !absolute.startsWith(resolve(tmpdir()) + sep) ||
      !basename(absolute).startsWith('aura-l18-credential-')
    )
      throw new Error('Unsafe test cleanup path');
    await rm(absolute, { recursive: true, force: true });
    testRootRemoved = true;
  } catch {
    errors.push(new Error('Isolated TEMP root cleanup could not be verified'));
  }
  console.log(
    JSON.stringify({
      credentialCleanupVerified,
      testRootRemoved,
      ...(testRootRemoved ? {} : { retainedRoot: root }),
    }),
  );
  return errors;
}

// eslint-disable-next-line no-empty-pattern -- real isolated Electron/Core and a local plugin; no model or external service.
test('plugin secret reaches Windows credentials and its service, stays out of logs, and is removed on uninstall', async ({}) => {
  test.setTimeout(300_000);
  test.skip(
    process.env.AURA_L18_REAL_CREDENTIAL_E2E !== '1',
    'Requires explicit owner consent to write and delete synthetic Windows credentials',
  );
  test.skip(!(await ownerIdle()), 'Owner must be idle for ten minutes and desktop unlocked');
  const name = `l18-credential-${randomBytes(6).toString('hex')}`;
  const owner = `plugin-setting:${name}/token`;
  const root = await mkdtemp(join(tmpdir(), 'aura-l18-credential-'));
  const seen = new Set<string>();
  let app: ElectronApplication | undefined;
  let failure: unknown;
  let cleanupErrors: Error[];
  try {
    // Capture the process before waiting for Core, so a readiness failure still closes it.
    app = await electron.launch({
      args: [`--user-data-dir=${join(root, 'Electron')}`, resolve(import.meta.dirname, '..')],
      env: {
        ...process.env,
        APPDATA: join(root, 'Roaming'),
        LOCALAPPDATA: join(root, 'Local'),
        AURA_MEMORY_TEST_ROOT: join(root, 'global-memories'),
        AURA_USER_PROFILE_TEST_ROOT: join(root, 'user-profile'),
        AURA_ALLOW_DEV_COMPONENT_MISMATCH: '1',
        AURA_E2E: '1',
        AURA_E2E_CORE_MODE: 'normal',
        AURA_P05_CLEAN_ACCEPTANCE: '1',
        AURA_TERMINAL_TEST_NO_PROFILE: '1',
        ELECTRON_DISABLE_SECURITY_WARNINGS: 'true',
      },
    });
    const page = await app.firstWindow();
    await expect(page.locator('.app-shell[data-core-state="ready"]')).toBeVisible({
      timeout: 60_000,
    });
    await addAndTrust(app, page, await plugin(root, name), name);
    const settings = await pluginSettings(page);
    const dialog = await pluginDialog(page, pluginRow(settings, name), name);
    const field = dialog.getByLabel('Token（必填）');
    await expect(field).toBeVisible();
    const value = `fixture-${randomBytes(32).toString('hex')}`;
    const expectedDigest = createHash('sha256').update(value).digest('hex');
    await field.evaluate((element, text) => {
      (element as HTMLInputElement).value = text;
    }, value);
    await dialog.getByRole('button', { name: '保存 Token', exact: true }).click();
    await expect(field).toHaveAttribute('placeholder', '已设置；留空保持不变');
    const targets = await registeredTargets(root, owner, seen);
    expect(targets.length === 1 && credentials(owner, targets, 'inspect') === 1).toBe(true);
    await dialog.getByRole('button', { name: '关闭', exact: true }).click();
    await configure(page, name, { service: true });
    const partitions = await readdir(join(root, 'Local', 'Aura', 'namespaces'));
    expect(partitions.length === 1).toBe(true);
    const local = join(root, 'Local', 'Aura', 'namespaces', partitions[0]!);
    const digest = join(local, 'plugins', 'data', name, 'received.digest');
    await expect
      .poll(async () => (await readFile(digest, 'utf8').catch(() => '')) === expectedDigest, {
        timeout: 30_000,
      })
      .toBe(true);
    const log = join(local, 'plugins', 'logs', `${name}.jsonl`);
    await expect
      .poll(async () => (await readFile(log, 'utf8').catch(() => '')).includes('••••'), {
        timeout: 30_000,
      })
      .toBe(true);
    const logs = (await files(root)).filter((file) => /\.(?:log|jsonl)$/iu.test(file));
    for (const file of logs) {
      const bytes = await readFile(file);
      expect(bytes.includes(Buffer.from(value))).toBe(false);
    }
    const updated = await pluginSettings(page);
    const details = await pluginDialog(page, pluginRow(updated, name), name);
    await details.getByRole('button', { name: '卸载', exact: true }).click();
    await page
      .getByRole('alertdialog', { name: `卸载 ${name}` })
      .getByRole('button', { name: '卸载', exact: true })
      .click();
    await expect.poll(() => credentials(owner, [...seen], 'inspect')).toBe(0);
  } catch (error) {
    failure = error;
  } finally {
    cleanupErrors = await cleanup(app, owner, root, seen);
  }
  if (failure !== undefined || cleanupErrors.length)
    throw new AggregateError(
      [...(failure === undefined ? [] : [failure]), ...cleanupErrors],
      'Credential acceptance or cleanup failed',
    );
});
