// @vitest-environment node
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { clipboard, dialog } from 'electron';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { handleMessagePicture } from './message-pictures.js';

// A message's picture is copied as a picture or saved where the user chooses;
// Main reads it from Core by the conversation's record, never takes bytes from the page.

vi.mock('electron', () => ({
  ipcMain: { handle: vi.fn() },
  clipboard: { write: vi.fn(async () => undefined) },
  ClipboardItem: class {
    constructor(readonly items: Record<string, unknown>) {}
  },
  dialog: { showSaveDialog: vi.fn() },
  nativeImage: {
    createFromBuffer: vi.fn(() => ({ isEmpty: () => false, toPNG: () => Buffer.from('png') })),
  },
}));

const session_id = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const artifact_id = 'art_01M5NMX46HWY85MHRGS06BMJ1N';
const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47]);
let base: string;

beforeEach(async () => {
  vi.clearAllMocks();
  base = await mkdtemp(join(tmpdir(), 'aura-l13-4-picture-'));
});
afterEach(async () => {
  await rm(base, { recursive: true, force: true });
});

const core = () => ({
  requestWithBudget: vi.fn(async () => ({
    media_type: 'image/png',
    data_base64: PNG.toString('base64'),
  })),
});

test('copy puts the picture Core keeps on the clipboard', async () => {
  const asked = core();
  expect(
    await handleMessagePicture(asked, undefined, {
      action: 'copy',
      session_id,
      artifact_id,
      name: 'shot.png',
    }),
  ).toEqual({ completed: true, message: '已复制图片' });
  expect(asked.requestWithBudget).toHaveBeenCalledWith(
    'browser.screenshot.read',
    { session_id, artifact_id },
    15_000,
  );
  expect(clipboard.write).toHaveBeenCalledTimes(1);
});

test('save writes the picture where the user chose, with the extension of its kind', async () => {
  const target = join(base, 'kept.png');
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({ canceled: false, filePath: target });
  expect(
    await handleMessagePicture(core(), undefined, {
      action: 'save',
      session_id,
      artifact_id,
      name: 'shot',
    }),
  ).toEqual({ completed: true, message: '已保存图片' });
  expect(vi.mocked(dialog.showSaveDialog).mock.calls[0]![0]).toMatchObject({
    defaultPath: 'shot.png',
  });
  expect(await readFile(target)).toEqual(PNG);
});

test('a cancelled save says nothing; a malformed request reads nothing', async () => {
  vi.mocked(dialog.showSaveDialog).mockResolvedValue({ canceled: true, filePath: '' });
  expect(
    await handleMessagePicture(core(), undefined, {
      action: 'save',
      session_id,
      artifact_id,
      name: 'shot.png',
    }),
  ).toEqual({ completed: false, message: '' });
  const asked = core();
  expect(
    await handleMessagePicture(asked, undefined, {
      action: 'copy',
      session_id,
      artifact_id,
      name: 'shot.png',
      bytes: 'iVBO',
    }),
  ).toEqual({ completed: false, message: '无效的图片请求' });
  expect(asked.requestWithBudget).not.toHaveBeenCalled();
});
