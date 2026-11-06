import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { MarkdownBlocks } from '../markdown-body';
import { MessageText } from './MessageText';

// A person types Windows paths. Read as Markdown, `\.`, `\_` or `\[` is an escaped character and
// the backslash in front of it disappeared, so `C:\Work\app\.cache` showed as
// `C:\Work\app.cache`. A person's own words show every backslash they typed; the model's
// answer stays Markdown.

beforeEach(() => {
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      observe() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const typed = (text: string) =>
  render(
    <MessageText
      preview={text}
      role="user"
      sessionId="ses_01M5PF3QWJN5S3P0JFPVBSFY3R"
      source={{ kind: 'user', event_id: 'evt_01M5PF3QWJN5S3P0JFPVBSFY3R' }}
      totalBytes={new TextEncoder().encode(text).length}
    />,
  );

test('a message the person typed shows every backslash of its paths', () => {
  const { container } = typed(
    '看一下 C:\\Work\\app\\.cache\\run_1\\[旧]\\notes.md，再看 D:\\资料\\_草稿\\*.txt',
  );
  expect(container.textContent).toContain('C:\\Work\\app\\.cache\\run_1\\[旧]\\notes.md');
  expect(container.textContent).toContain('D:\\资料\\_草稿\\*.txt');
});

test('a backslash at the end of a typed line stays, and the line breaks once', () => {
  const { container } = typed('目录是 D:\\报告\\\n第二行');
  expect(container.textContent).toContain('D:\\报告\\');
  expect(container.textContent).toContain('第二行');
  expect(container.querySelectorAll('br').length).toBe(1);
});

test('the rest of Markdown still works in what the person typed', () => {
  const { container } = typed('**重点**在 `C:\\x\\.y` 里');
  expect(container.querySelector('[data-streamdown="strong"]')?.textContent).toBe('重点');
  expect(container.querySelector('[data-streamdown="inline-code"]')?.textContent).toBe('C:\\x\\.y');
});

test('a queued message the person typed keeps its backslashes too', () => {
  const { container } = render(
    <MarkdownBlocks programs text={'改 C:\\Work\\app\\.cache 下的文件'} />,
  );
  expect(container.textContent).toContain('C:\\Work\\app\\.cache');
});

test('the model’s answer is Markdown: a backslash before punctuation escapes it', () => {
  const { container } = render(<MarkdownBlocks text={'\\*不是强调\\* 和 a\\_b'} />);
  expect(container.textContent).toContain('*不是强调* 和 a_b');
  expect(container.querySelector('em')).toBeNull();
});
