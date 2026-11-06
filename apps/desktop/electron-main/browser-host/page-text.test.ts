// @vitest-environment node
import { describe, expect, it, vi } from 'vitest';

import { PAGE_TEXT_CHARS, pageText, type TextContents } from './page-text.js';

const contents = (text: unknown, overrides: Partial<TextContents> = {}): TextContents => ({
  isDestroyed: () => false,
  getTitle: () => 'Example',
  getURL: () => 'https://example.com/a',
  executeJavaScriptInIsolatedWorld: vi.fn().mockResolvedValue(text),
  ...overrides,
});

describe('a page read for a message that refers to it', () => {
  it("gives its title, address and text, read apart from the page's own scripts", async () => {
    const page = contents('Hello\nWorld');
    expect(await pageText(page)).toEqual({
      title: 'Example',
      url: 'https://example.com/a',
      text: 'Hello\nWorld',
      truncated: false,
    });
    const [world, scripts] = vi.mocked(page.executeJavaScriptInIsolatedWorld).mock.calls[0]!;
    expect(world).toBeGreaterThan(0);
    expect(scripts[0]!.code).toContain('innerText');
  });

  it('keeps the first part of a long page and says so', async () => {
    const long = 'x'.repeat(PAGE_TEXT_CHARS + 10);
    const read = await pageText(contents(long));
    expect(read?.text.length).toBe(PAGE_TEXT_CHARS);
    expect(read?.truncated).toBe(true);
  });

  it('gives nothing for a closed page, and an empty text for one with none', async () => {
    expect(await pageText(undefined)).toBeUndefined();
    expect(await pageText(contents('', { isDestroyed: () => true }))).toBeUndefined();
    expect((await pageText(contents(null)))?.text).toBe('');
    const failing = contents('', {
      executeJavaScriptInIsolatedWorld: vi.fn().mockRejectedValue(new Error('crashed')),
    });
    expect(await pageText(failing)).toBeUndefined();
  });
});
