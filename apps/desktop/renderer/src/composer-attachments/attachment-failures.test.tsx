import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { MAX_IMAGE_BYTES, readAttachment } from './attachment-files';
import { useComposerAttachments } from './useComposerAttachments';

// A file that cannot be added says which file, why and what to do; the rest of what was dropped,
// pasted or picked with it is still added.

const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]);
const place = (file: File) => `C:/work/${file.name}`;

beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { attachmentPath: place } },
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** A file the system no longer gives: locked, deleted after choosing, or a cloud placeholder. */
function unreadable(name: string, type = 'text/plain'): File {
  const file = new File(['x'], name, { type });
  const refuse = () =>
    Promise.reject(new DOMException('The file could not be read.', 'NotReadableError'));
  Object.defineProperty(file, 'text', { value: refuse });
  Object.defineProperty(file, 'arrayBuffer', { value: refuse });
  return file;
}

test('a file that cannot be read is named, and the files dropped with it are still added', async () => {
  const { result } = renderHook(() => useComposerAttachments());
  await act(() =>
    result.current.add([
      unreadable('locked.txt'),
      new File(['buy milk'], 'todo.txt', { type: 'text/plain' }),
      unreadable('cloud.png', 'image/png'),
    ]),
  );
  expect(result.current.attachments.map((attachment) => attachment.name)).toEqual(['todo.txt']);
  expect(result.current.error).toBe(
    '「locked.txt」读不出来（可能已被移动、删除或正被其他程序占用），没有附加。确认文件能打开后再附加。 ' +
      '「cloud.png」读不出来（可能已被移动、删除或正被其他程序占用），没有附加。确认文件能打开后再附加。',
  );
});

test('a picture that cannot be decoded says so apart from one still over 3 MB once redrawn', async () => {
  vi.stubGlobal('createImageBitmap', async () => {
    throw new DOMException('The source image could not be decoded.', 'InvalidStateError');
  });
  expect(await readAttachment(new File([PNG], 'bad.png', { type: 'image/png' }), place)).toEqual({
    error:
      '「bad.png」不能作为图片附加：无法识别这个图片格式。没有附加；可以另存为 PNG 或 JPEG 再试。',
  });
  vi.stubGlobal('createImageBitmap', async () => ({ width: 8000, height: 6000, close: () => {} }));
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockReturnValue(null);
  vi.spyOn(HTMLCanvasElement.prototype, 'toBlob').mockImplementation(function (callback) {
    callback(new Blob([new Uint8Array(MAX_IMAGE_BYTES + 1)]));
  });
  expect(await readAttachment(new File([PNG], 'huge.png', { type: 'image/png' }), place)).toEqual({
    error: '「huge.png」压缩后仍超过 3 MB，没有附加。请缩小图片后再试。',
  });
});
