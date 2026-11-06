import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { MarkdownBlocks } from '../markdown-body';

// The model writes Windows paths outside code formatting too. Read as Markdown, `\.`, `\_`, `\[`
// or `\$` is an escaped character and the backslash in front of it disappeared, so
// `C:\Work\app\.cache` showed as `C:\Work\app.cache`. Inside a path every backslash shows;
// everywhere else the model's escapes still work.

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

const answer = (text: string) => render(<MarkdownBlocks text={text} />).container;

test('a path in the model’s answer shows every backslash', () => {
  const shown = answer(
    '结果在 C:\\Work\\app\\.cache\\runs\\r18-18 里，草稿在 D:\\资料\\_草稿\\[旧]\\$Recycle.Bin。',
  ).textContent;
  expect(shown).toContain('C:\\Work\\app\\.cache\\runs\\r18-18 里');
  expect(shown).toContain('D:\\资料\\_草稿\\[旧]\\$Recycle.Bin。');
});

test('a path with spaces, Chinese folders or a network share keeps its backslashes', () => {
  const shown = answer(
    '见 C:\\Program Files\\.config\\x、C:\\Users\\张三\\.ssh\\config，以及 \\\\nas\\share\\.snapshots',
  ).textContent;
  expect(shown).toContain('C:\\Program Files\\.config\\x、');
  expect(shown).toContain('C:\\Users\\张三\\.ssh\\config，');
  expect(shown).toContain('\\\\nas\\share\\.snapshots');
});

test('a path the model wrote with doubled backslashes shows single ones', () => {
  expect(answer('打开 C:\\\\Users\\\\foo\\\\.git 看看').textContent).toContain(
    'C:\\Users\\foo\\.git 看看',
  );
});

test('outside paths the model’s escapes still work, in the same paragraph', () => {
  const container = answer('**注意** \\*号不是强调\\*，路径是 C:\\a\\.b，a\\_b 照旧');
  expect(container.textContent).toContain('*号不是强调*，路径是 C:\\a\\.b，a_b 照旧');
  expect(container.querySelector('em')).toBeNull();
  expect(container.querySelector('[data-streamdown="strong"]')?.textContent).toBe('注意');
});

test('a path in bold, on a second line, or in code shows every backslash', () => {
  const container = answer('**C:\\Dev\\.aura** 和\nE:\\work\\.cache 还有 `C:\\x\\.y`');
  expect(container.querySelector('[data-streamdown="strong"]')?.textContent).toBe('C:\\Dev\\.aura');
  expect(container.textContent).toContain('E:\\work\\.cache');
  expect(container.querySelector('[data-streamdown="inline-code"]')?.textContent).toBe('C:\\x\\.y');
});

test('a link to a local file keeps the folders of its path', () => {
  const container = answer('写好了：[报告](C:\\Dev\\.aura-dev\\r.md)');
  expect(container.querySelector('[data-streamdown="inline-code"]')?.textContent).toBe(
    'C:/Dev/.aura-dev/r.md',
  );
});
