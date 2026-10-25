import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './files/file-links';
import { root, session, workspace } from './files/files-fixture';
import { MarkdownBody } from './markdown-body';

// An address found in the text ends where Chinese punctuation or text
// begins; an address written as inline code is a link too; an email is not.

const openExternalLink = vi.fn();
beforeEach(() => {
  openExternalLink.mockReset().mockResolvedValue({ ok: true, value: { completed: true } });
  vi.stubGlobal('aura', { v1: { openExternalLink } });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const said = (text: string) =>
  render(
    <FileLinkContext.Provider value={{ session, workspace, root, notify: vi.fn() }}>
      <MarkdownBody text={text} />
    </FileLinkContext.Provider>,
  );
const body = () => document.querySelector('.markdown-body') as HTMLElement;
const links = () =>
  [...document.querySelectorAll<HTMLAnchorElement>('.markdown-body a')].map((a) => ({
    text: a.textContent,
    host: new URL(a.href).host,
    href: a.getAttribute('href'),
  }));

test('an address followed by Chinese punctuation ends before it', () => {
  said('我已在默认浏览器中启动 www.deepl.com（通过 Windows 的 Start-Process，退出码为 0）。');
  expect(links()).toEqual([
    { text: 'www.deepl.com', host: 'www.deepl.com', href: 'https://www.deepl.com/' },
  ]);
  expect(body().textContent).toBe(
    '我已在默认浏览器中启动 www.deepl.com（通过 Windows 的 Start-Process，退出码为 0）。',
  );
  expect(body().textContent).not.toContain('xn--');
});

test('a full address before a Chinese comma opens that address and is shown once', () => {
  said('已用 Aura 内置浏览器打开 https://www.deepl.com，页面加载成功。');
  expect(links()).toEqual([
    { text: 'https://www.deepl.com', host: 'www.deepl.com', href: 'https://www.deepl.com/' },
  ]);
  expect(body().textContent).toBe('已用 Aura 内置浏览器打开 https://www.deepl.com，页面加载成功。');
});

test('a host followed directly by Chinese ends at the host; a Chinese path is kept', () => {
  said('访问www.example.com获取，或看 https://zh.wikipedia.org/wiki/中文 这一页');
  expect(links().map((link) => link.text)).toEqual([
    'www.example.com',
    'https://zh.wikipedia.org/wiki/中文',
  ]);
  expect(body().textContent).toContain('获取');
});

test('an email address is not a link', () => {
  said('有问题发到 someone@example.com 或 mailto:team@example.com。');
  expect(links()).toEqual([]);
  expect(body().textContent).toBe('有问题发到 someone@example.com 或 mailto:team@example.com。');
});

test('an address written as inline code is a link; other code stays code', async () => {
  const user = userEvent.setup();
  said(
    '实际地址：`https://www.deepl.com/en`（站点自动跳转），阻止的是 `www.deepseek.com`，命令 `npm run dev`。',
  );
  const opened = links();
  expect(opened.map((link) => link.href)).toEqual([
    'https://www.deepl.com/en',
    'https://www.deepseek.com',
  ]);
  expect(body().querySelectorAll('a code')).toHaveLength(2);
  expect(screen.getByText('npm run dev').closest('a')).toBeNull();
  await user.click(screen.getByText('https://www.deepl.com/en'));
  expect(openExternalLink).toHaveBeenLastCalledWith({ url: 'https://www.deepl.com/en' });
});

// Real check: with no space after an address, GFM read on into the code span after it.
test('code written right after an address in Chinese text is still code, and its address a link', () => {
  said(
    '已用 Aura 内置浏览器打开 https://www.deepl.com，页面加载成功。实际地址：`https://www.deepl.com/en`。联系 someone@example.com。',
  );
  expect(links().map((link) => link.href)).toEqual([
    'https://www.deepl.com/',
    'https://www.deepl.com/en',
  ]);
  expect(body().querySelector('a code')?.textContent).toBe('https://www.deepl.com/en');
  expect(body().textContent).not.toContain('`');
});
