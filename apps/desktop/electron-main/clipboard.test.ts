// @vitest-environment node
import { clipboard } from 'electron';
import { beforeEach, expect, test, vi } from 'vitest';
import { writeClipboard } from './clipboard.js';

// Main refuses
// the browser's clipboard permissions to every page, so what the user copies is written here.

vi.mock('electron', () => ({ clipboard: { writeText: vi.fn() } }));
beforeEach(() => vi.clearAllMocks());

test('the text the user copied is written to the clipboard as it is', () => {
  const text = '你好！👋\n\n- 第一行\n- `code`\r\n结尾';
  expect(writeClipboard({ text })).toEqual({ completed: true, message: '已复制' });
  expect(clipboard.writeText).toHaveBeenCalledExactlyOnceWith(text);
});

test('nothing larger than 16 MiB is written, and the user is told why', () => {
  const text = '复'.repeat(6 * 1024 * 1024);
  expect(writeClipboard({ text })).toEqual({
    completed: false,
    message: '内容太大（超过 16 MB），没有复制。',
  });
  expect(clipboard.writeText).not.toHaveBeenCalled();
});

test('a clipboard that refuses is said plainly, never thrown at the renderer', () => {
  vi.mocked(clipboard.writeText).mockImplementationOnce(() => {
    throw new Error('OpenClipboard failed');
  });
  expect(writeClipboard({ text: 'x' })).toEqual({
    completed: false,
    message: '剪贴板暂时不可用（可能正被其他程序占用），请重试。',
  });
});
