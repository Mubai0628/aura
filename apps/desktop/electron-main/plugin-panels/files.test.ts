// @vitest-environment node
import { link, mkdir, mkdtemp, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, expect, test } from 'vitest';
import { MAX_PANEL_FILE_BYTES, panelFile, panelSegments } from './files.js';

const roots: string[] = [];
afterEach(async () => {
  await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true })));
});

async function folder(prefix: string): Promise<string> {
  const root = await mkdtemp(join(tmpdir(), prefix));
  roots.push(root);
  return root;
}

/** A plugin folder with a panel, and a file of the plugin's outside it. */
async function plugin(): Promise<{ root: string; panel: string }> {
  const root = await folder('aura-panel-files-');
  const panel = join(root, 'panel');
  await mkdir(join(panel, 'lib'), { recursive: true });
  await writeFile(join(panel, 'index.html'), '<!doctype html><p>hi</p>');
  await writeFile(join(panel, 'lib', 'app.js'), 'export {};');
  await writeFile(join(root, 'secret.txt'), 'outside the panel');
  return { root, panel };
}

test('a panel loads its own files under panel/, each with its type', async () => {
  const { panel } = await plugin();
  const page = await panelFile(panel, '/index.html');
  expect(page?.type).toBe('text/html; charset=utf-8');
  expect(page?.body.toString()).toContain('<p>hi</p>');
  expect((await panelFile(panel, '/lib/app.js'))?.type).toBe('text/javascript; charset=utf-8');
  // An encoded name is read as the name it spells.
  await writeFile(join(panel, 'a b.css'), 'p {}');
  expect((await panelFile(panel, '/a%20b.css'))?.type).toBe('text/css; charset=utf-8');
});

test('nothing outside panel/ is reached, however the address spells it', async () => {
  const { panel } = await plugin();
  for (const pathname of [
    '/../secret.txt',
    '/%2e%2e/secret.txt',
    '/lib/..%2f..%2fsecret.txt',
    '/..%5csecret.txt',
    '/lib%5c..%5c..%5csecret.txt',
    '/C:%5cWindows%5cwin.ini',
    '/./index.html',
    '//index.html',
    'index.html',
    '/index.html.',
    '/index.html%20',
    '/con.txt',
    '/lib/NUL.js',
    '/index.html%3A%3A%24DATA',
    '/%E0%A4%A.txt',
    '/a%00.js',
  ])
    expect(await panelFile(panel, pathname), pathname).toBeUndefined();
  expect(panelSegments(`/${Array.from({ length: 17 }, () => 'd').join('/')}.js`)).toBeUndefined();
});

test('only files of a known type are served, never a folder or a missing file', async () => {
  const { panel } = await plugin();
  await writeFile(join(panel, 'tool.exe'), 'MZ');
  await writeFile(join(panel, 'notes'), 'no type');
  await mkdir(join(panel, 'folder.js'));
  for (const pathname of ['/tool.exe', '/notes', '/folder.js', '/missing.js', '/lib'])
    expect(await panelFile(panel, pathname), pathname).toBeUndefined();
});

test('a file larger than a panel may load is not served', async () => {
  const { panel } = await plugin();
  await writeFile(join(panel, 'big.js'), Buffer.alloc(MAX_PANEL_FILE_BYTES + 1));
  expect(await panelFile(panel, '/big.js')).toBeUndefined();
});

// A link is followed nowhere: a junction inside panel/, a panel folder that is itself one, and a
// hard link that gives a file kept elsewhere a name inside the panel.
test('no link takes the panel to a file kept elsewhere', async () => {
  const { panel } = await plugin();
  const outside = await folder('aura-panel-outside-');
  await writeFile(join(outside, 'leak.js'), 'leak');
  await symlink(outside, join(panel, 'linked'), 'junction');
  expect(await panelFile(panel, '/linked/leak.js')).toBeUndefined();

  const other = await folder('aura-panel-other-');
  await symlink(panel, join(other, 'panel'), 'junction');
  expect(await panelFile(join(other, 'panel'), '/index.html')).toBeUndefined();

  await link(join(outside, 'leak.js'), join(panel, 'hard.js'));
  expect(await panelFile(panel, '/hard.js')).toBeUndefined();
  // The same folder still serves its own files.
  expect(await panelFile(panel, '/index.html')).toBeDefined();
});
