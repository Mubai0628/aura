import { cleanup, render } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { MarkdownBlocks, MarkdownBody } from './markdown-body';

afterEach(cleanup);

function body(): HTMLElement {
  return document.querySelector('.markdown-body') as HTMLElement;
}

test('a paragraph becomes a real paragraph, not one flat text node', () => {
  render(<MarkdownBody text={'你好\n\n第二段'} />);
  expect(document.querySelectorAll('.markdown-body p')).toHaveLength(2);
});

test('strong, emphasis and inline code become real elements', () => {
  render(<MarkdownBody text={'**粗体** 与 *斜体* 与 `code`'} />);
  // Streamdown renders emphasis through its own styled elements and marks each
  // one, so the assertion follows its contract rather than a bare tag name.
  expect(body().querySelector('[data-streamdown="strong"]')?.textContent).toBe('粗体');
  expect(body().querySelector('em')?.textContent).toBe('斜体');
  expect(body().querySelector('[data-streamdown="inline-code"]')?.textContent).toBe('code');
});

/**
 * The cases that made the previous hand-written parser unsuitable. Each input is
 * something a model really emits, and each produced a wrong result before.
 */
test('a trailing backslash is not replaced by a placeholder', () => {
  render(<MarkdownBody text={'path\\'} />);
  expect(body().textContent).toBe('path\\');
  expect(body().textContent).not.toContain('undefined');
});

test('emphasis next to CJK text is parsed', () => {
  render(<MarkdownBody text={'**完成**后继续'} />);
  expect(body().querySelector('[data-streamdown="strong"]')?.textContent).toBe('完成');
  expect(body().textContent).toBe('完成后继续');
});

test('an escaped pipe stays inside one table cell', () => {
  render(<MarkdownBody text={'| a | b |\n| --- | --- |\n| x \\| y | z |\n'} />);
  const cells = body().querySelectorAll('td');
  expect(cells).toHaveLength(2);
  expect(cells[0]?.textContent).toBe('x | y');
  expect(cells[1]?.textContent).toBe('z');
});

test('a long document renders without throwing', () => {
  const lines = Array.from({ length: 10_001 }, (_, index) => `line ${index}`).join('\n\n');
  render(<MarkdownBody text={lines} />);
  expect(body().querySelectorAll('p').length).toBeGreaterThan(10_000);
});

test('raw HTML is never a live element', () => {
  render(<MarkdownBody text={'<b>not bold</b> and <script>alert(1)</script>'} />);
  expect(body().querySelector('b, script, iframe, img')).toBeNull();
  // Without the raw pass the source HTML is text, not markup that was filtered.
  expect(body().textContent).toBe('<b>not bold</b> and <script>alert(1)</script>');
});

test('raw HTML is inert even for a tag Markdown itself can produce', () => {
  render(<MarkdownBody text={'<strong>raw strong</strong>'} />);
  // `strong` is in the allowed set for real Markdown, so this only holds because
  // the raw pass is gone rather than because the tag was filtered out.
  expect(body().querySelector('strong')).toBeNull();
  expect(body().querySelector('[data-streamdown="strong"]')).toBeNull();
  expect(body().textContent).toBe('<strong>raw strong</strong>');
});

test('an event-handler attribute in raw HTML stays text', () => {
  render(<MarkdownBody text={'<img src=x onerror="alert(1)">'} />);
  expect(body().querySelector('img')).toBeNull();
  expect(body().querySelector('[onerror]')).toBeNull();
  expect(body().textContent).toBe('<img src=x onerror="alert(1)">');
});

test('a link shows its text and its full address without becoming a control', () => {
  render(<MarkdownBody text={'see [docs](https://example.com/a?x=1&y=2)'} />);
  expect(body().querySelectorAll('a')).toHaveLength(0);
  expect(body().textContent).toBe('see docs（https://example.com/a?x=1&y=2）');
  // The address is isolated so it keeps its direction and wraps as a unit.
  const url = body().querySelector('.markdown-link-url');
  expect(url?.getAttribute('dir')).toBe('ltr');
  expect(url?.textContent).toBe('https://example.com/a?x=1&y=2');
});

test('a link whose label is already the address prints it once', () => {
  render(<MarkdownBody text={'<https://example.com/a>'} />);
  expect(body().textContent).toBe('https://example.com/a');
});

test('a bare URL autolinks and prints once', () => {
  render(<MarkdownBody text={'see https://example.com/a for details'} />);
  expect(body().textContent).toBe('see https://example.com/a for details');
});

test('a formatted label keeps its formatting next to the address', () => {
  render(<MarkdownBody text={'[**文档** 与 `code`](https://example.com/a)'} />);
  expect(body().querySelector('[data-streamdown="strong"]')?.textContent).toBe('文档');
  expect(body().querySelector('[data-streamdown="inline-code"]')?.textContent).toBe('code');
  expect(body().textContent).toBe('文档 与 code（https://example.com/a）');
});

test('a blocked link protocol keeps its label and gains no address', () => {
  render(<MarkdownBody text={'[bad](javascript:alert%281%29)'} />);
  expect(body().querySelector('a')).toBeNull();
  // The words alone, without an English "[blocked]" after them.
  expect(body().textContent).toBe('bad');
});

test('an unfinished link target shows no placeholder address', () => {
  render(<MarkdownBody text={'[文档](https://'} streaming />);
  expect(body().textContent).toBe('文档');
  expect(body().textContent).not.toContain('streamdown');
  expect(body().querySelector('a')).toBeNull();
});

test('a link inside a code span stays literal source', () => {
  render(<MarkdownBody text={'`[文档](https://example.com)`'} />);
  expect(body().textContent).toBe('[文档](https://example.com)');
});

test('a link shows its address while it is still streaming', () => {
  render(<MarkdownBody text={'见 [文档](https://example.com/a?x=1&y=2) 结尾'} streaming />);
  expect(body().textContent).toBe('见 文档（https://example.com/a?x=1&y=2） 结尾');
  expect(body().querySelector('a')).toBeNull();
  expect(body().querySelector('.markdown-link-url')?.textContent).toBe(
    'https://example.com/a?x=1&y=2',
  );
});

test('raw HTML stays text while streaming', () => {
  render(<MarkdownBody text={'text\n\n<b>raw</b>'} streaming />);
  expect(body().querySelector('b')).toBeNull();
  expect(body().textContent).toContain('<b>raw</b>');
});

test('a reference link resolves in a finished body', () => {
  render(<MarkdownBody text={'[文档][ref]\n\n[ref]: https://example.com/ref'} />);
  expect(body().textContent).toBe('文档（https://example.com/ref）');
});

/**
 * A known gap this batch does not fix: a reference definition that arrives in a
 * later chunk is not resolved while streaming. The default renderer behaves the
 * same way, so it is a block-scope property of streaming mode and not something
 * the link component caused. It is kept as a failing-expectation test so the
 * behaviour cannot regress silently and cannot be reported as full support.
 */
test('a reference link defined in a later chunk is not resolved while streaming', () => {
  render(<MarkdownBody text={'[文档][ref]\n\n[ref]: https://example.com/ref'} streaming />);
  expect(body().textContent).toBe('[文档][ref]');
  expect(body().querySelector('a')).toBeNull();
});

test('a fenced code block keeps its content literal', () => {
  render(<MarkdownBody text={'```\n**not bold**\n```'} />);
  expect(body().querySelector('strong')).toBeNull();
  expect(body().textContent).toContain('**not bold**');
});

test('a nested list and a blockquote render as structure', () => {
  render(<MarkdownBody text={'- one\n  - nested\n\n> quoted\n'} />);
  expect(body().querySelectorAll('ul').length).toBeGreaterThanOrEqual(2);
  expect(body().querySelector('blockquote')?.textContent).toContain('quoted');
});

test('an unterminated fence renders while streaming instead of breaking', () => {
  render(<MarkdownBody text={'text\n\n```rust\nfn main() {'} streaming />);
  // The partial construct must not swallow the prose that precedes it.
  expect(body().textContent).toContain('text');
  expect(body().textContent).toContain('fn main() {');
});

test('a table split across two chunks ends up as one table', () => {
  const { rerender } = render(<MarkdownBody text={'| a | b |'} streaming />);
  rerender(<MarkdownBody text={'| a | b |\n| --- | --- |\n| 1 | 2 |\n'} streaming />);
  const cells = body().querySelectorAll('td');
  expect(cells.length).toBe(2);
  expect(cells[0]?.textContent).toBe('1');
});

test('the caller keeps its own class on the container', () => {
  render(<MarkdownBlocks text="body" className="agent-final-report" />);
  expect(body().className).toBe('markdown-body agent-final-report');
});
