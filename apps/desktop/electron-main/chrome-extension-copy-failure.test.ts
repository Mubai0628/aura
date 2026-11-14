// @vitest-environment node
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { app, shell } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { chromeExtensionCopy, openChromeExtensionFolder } from './chrome-extension-folder.js';

// A copy of the extension that could not be made or brought up to date is not opened as it is:
// the settings page says where it should be, why it is not ready and what to do.

const fsControl = vi.hoisted(() => ({ refuseCopy: undefined as string | undefined }));

vi.mock('node:fs/promises', async (importOriginal) => {
  const actual = await importOriginal<typeof import('node:fs/promises')>();
  return {
    ...actual,
    cp: async (...args: Parameters<typeof actual.cp>) => {
      if (fsControl.refuseCopy !== undefined)
        throw Object.assign(new Error(`${fsControl.refuseCopy}: refused`), {
          code: fsControl.refuseCopy,
        });
      return actual.cp(...args);
    },
  };
});

vi.mock('electron', () => ({
  app: { isPackaged: true, getAppPath: vi.fn(() => '') },
  shell: { openPath: vi.fn(async () => '') },
}));

const roots: string[] = [];
const resources = process.resourcesPath;

async function root(): Promise<string> {
  const made = await mkdtemp(join(tmpdir(), 'aura-chrome-extension-copy-'));
  roots.push(made);
  return made;
}

beforeEach(async () => {
  vi.clearAllMocks();
  const installed = await root();
  Object.defineProperty(process, 'resourcesPath', { configurable: true, value: installed });
  await mkdir(join(installed, 'chrome-extension'), { recursive: true });
  await writeFile(join(installed, 'chrome-extension', 'manifest.json'), '{"version":"0.3.0"}');
});
afterEach(async () => {
  fsControl.refuseCopy = undefined;
  Object.defineProperty(process, 'resourcesPath', { configurable: true, value: resources });
  await Promise.all(roots.splice(0).map((made) => rm(made, { recursive: true, force: true })));
});

test('a copy that could not be made says why and is not opened', async () => {
  const data = await root();
  const copy = chromeExtensionCopy(data);
  fsControl.refuseCopy = 'ENOSPC';
  await expect(openChromeExtensionFolder(data)).rejects.toMatchObject({
    envelope: {
      code: 'ipc.chrome_extension_copy_failed',
      message: `没能把 Chrome 扩展复制到 ${copy}（磁盘已满），扩展文件夹没有准备好；关闭 Chrome 后再点一次“打开扩展文件夹”。`,
    },
  });
  expect(shell.openPath).not.toHaveBeenCalled();
  expect(app.isPackaged).toBe(true);
});

test('an outdated copy that could not be brought up to date is not opened as it is', async () => {
  const data = await root();
  const copy = chromeExtensionCopy(data);
  await mkdir(copy, { recursive: true });
  await writeFile(join(copy, 'manifest.json'), '{"version":"0.2.0"}');
  fsControl.refuseCopy = 'EBUSY';
  await expect(openChromeExtensionFolder(data)).rejects.toMatchObject({
    envelope: {
      code: 'ipc.chrome_extension_copy_failed',
      message: `没能把 Chrome 扩展复制到 ${copy}（文件正被其他程序使用，可能是 Chrome），扩展文件夹没有准备好；关闭 Chrome 后再点一次“打开扩展文件夹”。`,
    },
  });
  expect(shell.openPath).not.toHaveBeenCalled();
  fsControl.refuseCopy = 'EIO';
  await expect(openChromeExtensionFolder(data)).rejects.toMatchObject({
    envelope: {
      message: `没能把 Chrome 扩展复制到 ${copy}（磁盘已满、没有写入权限或文件正被 Chrome 使用），扩展文件夹没有准备好；关闭 Chrome 后再点一次“打开扩展文件夹”。`,
    },
  });
});
