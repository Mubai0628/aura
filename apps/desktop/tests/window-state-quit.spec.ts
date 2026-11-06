import { expect, test, type ElectronApplication } from '@playwright/test';
import { spawnSync } from 'node:child_process';
import { mkdtemp, readdir, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launchAuraWithDataRoot } from './electron-shell/harness.js';

// Main's app.quit() with the window still open, as the live-test tools end Aura: the window's
// close event comes in Electron's second quit pass, after Main's quit handler has stopped waiting
// for writes, and the exit can cut that write off before its rename.

const desktopFolder = async (root: string): Promise<string> => {
  const namespaces = join(root, 'Aura', 'namespaces');
  const [namespace, ...others] = await readdir(namespaces);
  if (namespace === undefined || others.length > 0) throw new Error('expected one namespace');
  return join(namespaces, namespace, 'desktop');
};

const windowStateFiles = async (desktop: string): Promise<string[]> =>
  (await readdir(desktop)).filter((name) => name.startsWith('window-state.')).sort();

const started = async (root: string): Promise<ElectronApplication> => {
  const application = await launchAuraWithDataRoot(root);
  const window = await application.firstWindow();
  await expect(window.locator('.app-shell[data-core-state="ready"]')).toBeVisible({
    timeout: 15_000,
  });
  return application;
};

const normalBounds = (application: ElectronApplication) =>
  application.evaluate(({ BrowserWindow }) => BrowserWindow.getAllWindows()[0]?.getNormalBounds());

const place = async (application: ElectronApplication, offset: number) => {
  await application.evaluate(({ BrowserWindow, screen }, by) => {
    const area = screen.getPrimaryDisplay().workArea;
    BrowserWindow.getAllWindows()[0]?.setBounds({
      height: 600,
      width: 900,
      x: area.x + by,
      y: area.y + by,
    });
  }, offset);
  return normalBounds(application);
};

const quitFromMain = async (application: ElectronApplication): Promise<void> => {
  const exited = new Promise((done) => application.process().once('exit', done));
  await application.evaluate(({ app }) => app.quit()).catch(() => undefined);
  await exited;
};

test('quitting with the window open keeps its place and leaves no temporary file', async () => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-window-state-quit-'));
  let application: ElectronApplication | undefined;
  try {
    let placed: Awaited<ReturnType<typeof normalBounds>>;
    for (let run = 1; run <= 3; run += 1) {
      application = await started(root);
      if (placed !== undefined) expect(await normalBounds(application)).toEqual(placed);
      placed = await place(application, 40 * run);
      await quitFromMain(application);
      application = undefined;
      const files = await windowStateFiles(await desktopFolder(root));
      expect(files, `after quit ${String(run)}`).toEqual(['window-state.v1.json']);
    }

    // What a process that ended mid-write leaves: the next start removes it.
    const desktop = await desktopFolder(root);
    const ended = spawnSync(process.execPath, ['-e', ''], { windowsHide: true }).pid;
    await writeFile(join(desktop, `window-state.v1.json.${String(ended)}.tmp`), '');
    application = await started(root);
    await expect.poll(() => windowStateFiles(desktop)).toEqual(['window-state.v1.json']);
  } finally {
    await application?.close().catch(() => undefined);
    await rm(root, { force: true, maxRetries: 3, recursive: true, retryDelay: 100 });
  }
});
