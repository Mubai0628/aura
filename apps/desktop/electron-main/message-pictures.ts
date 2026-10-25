import { writeFile } from 'node:fs/promises';
import { basename, extname } from 'node:path';
import {
  BrowserWindow,
  clipboard,
  ClipboardItem,
  dialog,
  ipcMain,
  nativeImage,
  type IpcMainInvokeEvent,
} from 'electron';
import {
  MESSAGE_PICTURE_CHANNEL,
  parseMessagePictureRequest,
  type MessagePictureResult,
} from '../shared/message-pictures.js';
import type { CoreSupervisor } from './core-supervisor.js';

type Core = Pick<CoreSupervisor, 'requestWithBudget'>;

const EXTENSIONS: Readonly<Record<string, string>> = { 'image/png': 'png', 'image/jpeg': 'jpg' };

/** The picture, as Core keeps it for the conversation. */
async function picture(
  core: Core,
  session_id: string,
  artifact_id: string,
): Promise<{ bytes: Buffer; extension: string } | undefined> {
  const value = (await core.requestWithBudget(
    'browser.screenshot.read',
    { session_id, artifact_id },
    15_000,
  )) as Record<string, unknown> | null;
  const extension = EXTENSIONS[String(value?.media_type)];
  if (extension === undefined || typeof value?.data_base64 !== 'string') return undefined;
  return { bytes: Buffer.from(value.data_base64, 'base64'), extension };
}

/** The name offered in the save dialog: the picture's own, with the extension of its kind. */
function offeredName(name: string, extension: string): string {
  // eslint-disable-next-line no-control-regex
  const plain = basename(name).replace(/[<>:"/\\|?*\u0000-\u001f]/gu, '_') || 'picture';
  return extname(plain).toLowerCase() === `.${extension}` ? plain : `${plain}.${extension}`;
}

/**
 * A message's picture, copied to the clipboard as a picture or saved where the
 * user chooses in the system's own dialog. Main reads it from Core by the conversation's record.
 */
export function installMessagePictures(
  core: () => Core | undefined,
  owner: () => BrowserWindow | undefined,
  assertTrustedSender: (event: IpcMainInvokeEvent) => void,
): void {
  ipcMain.handle(MESSAGE_PICTURE_CHANNEL, async (event, value: unknown) => {
    assertTrustedSender(event);
    return handleMessagePicture(core(), owner(), value);
  });
}

export async function handleMessagePicture(
  core: Core | undefined,
  window: BrowserWindow | undefined,
  value: unknown,
): Promise<MessagePictureResult> {
  const request = parseMessagePictureRequest(value);
  if (request === undefined) return { completed: false, message: '无效的图片请求' };
  if (core === undefined) return { completed: false, message: 'Aura Core 尚未就绪，请稍后再试' };
  let found: Awaited<ReturnType<typeof picture>>;
  try {
    found = await picture(core, request.session_id, request.artifact_id);
  } catch {
    found = undefined;
  }
  if (found === undefined) return { completed: false, message: '这张图片已不可用。' };
  if (request.action === 'copy') {
    // The clipboard takes a PNG: a JPEG is drawn again as one.
    const image = nativeImage.createFromBuffer(found.bytes);
    if (image.isEmpty()) return { completed: false, message: '这张图片已不可用。' };
    const png = new Uint8Array(image.toPNG());
    try {
      await clipboard.write([
        new ClipboardItem({ 'image/png': new Blob([png], { type: 'image/png' }) }),
      ]);
    } catch {
      return { completed: false, message: '剪贴板暂时不可用（可能正被其他程序占用），请重试。' };
    }
    return { completed: true, message: '已复制图片' };
  }
  const options = {
    title: '保存图片',
    defaultPath: offeredName(request.name, found.extension),
    filters: [{ name: found.extension.toUpperCase(), extensions: [found.extension] }],
  };
  const chosen =
    window === undefined
      ? await dialog.showSaveDialog(options)
      : await dialog.showSaveDialog(window, options);
  // Cancelled: nothing to say.
  if (chosen.canceled || !chosen.filePath) return { completed: false, message: '' };
  try {
    await writeFile(chosen.filePath, found.bytes);
  } catch {
    return { completed: false, message: '没能保存图片，请换一个位置再试。' };
  }
  return { completed: true, message: '已保存图片' };
}
