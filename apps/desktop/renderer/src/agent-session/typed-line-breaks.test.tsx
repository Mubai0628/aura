import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { MarkdownBlocks } from '../markdown-body';
import { MessageText } from './MessageText';

// A person who presses Enter once means a new line. Read as Markdown, a single line break inside
// a paragraph is a space, so three typed lines showed as one. A person's own words keep every line
// they typed; the model's answer stays Markdown.

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

/** Each line of a paragraph as it shows: the text between its line breaks. */
const lines = (paragraph: Element | null | undefined) =>
  [...(paragraph?.childNodes ?? [])]
    .reduce<string[]>(
      (shown, node) => {
        if (node.nodeName === 'BR') shown.push('');
        else shown[shown.length - 1] += node.textContent ?? '';
        return shown;
      },
      [''],
    )
    .map((line) => line.trim());

test('each line the person typed shows on its own line', () => {
  const { container } = typed('第一行\n第二行\n  第三行 **重点**');
  const paragraphs = container.querySelectorAll('p');
  expect(paragraphs.length).toBe(1);
  expect(lines(paragraphs[0])).toEqual(['第一行', '第二行', '第三行 重点']);
  expect(container.querySelector('[data-streamdown="strong"]')?.textContent).toBe('重点');
});

test('a blank line still starts a new paragraph, and lists and code keep their own lines', () => {
  const { container } = typed('第一段\n\n第二段\n\n- 甲\n- 乙\n\n```\nx = 1\ny = 2\n```');
  expect([...container.querySelectorAll('p')].map((p) => p.textContent)).toEqual([
    '第一段',
    '第二段',
  ]);
  expect(container.querySelectorAll('li').length).toBe(2);
  expect(container.querySelectorAll('br').length).toBe(0);
  const code = container.querySelector('pre')?.textContent;
  expect(code).toContain('x = 1');
  expect(code).toContain('y = 2');
});

test('a queued message the person typed keeps its lines too', () => {
  const { container } = render(<MarkdownBlocks programs text={'先做这个\n再做那个'} />);
  expect(lines(container.querySelector('p'))).toEqual(['先做这个', '再做那个']);
});

test('the model’s answer is Markdown: a single line break inside a paragraph is a space', () => {
  const { container } = render(<MarkdownBlocks text={'第一行\n第二行'} />);
  expect(container.querySelector('br')).toBeNull();
  expect(container.querySelector('p')?.textContent).toBe('第一行\n第二行');
});
