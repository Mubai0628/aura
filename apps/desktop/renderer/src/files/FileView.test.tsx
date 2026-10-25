import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileView } from './FileView';
import { installFilesApi, root, session, workspace } from './files-fixture';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// A file read only — text with line numbers, Markdown as a preview with its
// source one button away, a picture as itself, anything else named by its type and size.

const scope = { session, workspace, root };
let api: ReturnType<typeof installFilesApi>;
beforeEach(() => {
  resetToolTabsForTests();
  api = installFilesApi();
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const lines = () =>
  [...document.querySelectorAll('.file-line')].map((line) => [
    line.querySelector('.file-line-number')?.textContent,
    line.querySelector('.file-line-text')?.textContent,
  ]);

test('a text file shows each line with its number', async () => {
  render(<FileView scope={scope} path="src/main.rs" visible />);
  await screen.findByText('fn main() {');
  expect(lines()).toEqual([
    ['1', 'fn main() {'],
    ['2', '    println!("hello");'],
    ['3', '}'],
  ]);
  expect(api.readWorkspaceText).toHaveBeenCalledWith({
    session_id: session,
    workspace_id: workspace,
    target: { path: 'src/main.rs' },
    range: { range_kind: 'full' },
    explicit_encoding: null,
  });
});

test('Markdown opens as a preview; one button shows its source and back', async () => {
  const user = userEvent.setup();
  render(<FileView scope={scope} path="README.md" visible />);
  expect(await screen.findByRole('heading', { level: 1, name: '演示' })).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '查看源码' }));
  expect(lines()[0]).toEqual(['1', '# 演示']);
  expect(screen.queryByRole('heading', { level: 1, name: '演示' })).toBeNull();
  await user.click(screen.getByRole('button', { name: '查看预览' }));
  expect(screen.getByRole('heading', { level: 1, name: '演示' })).toBeTruthy();
});

// A picture, a video or a sound streams from
// an address Main gives, whatever its size; one the player cannot play opens with its program.
test('a picture shows from the address Main streams it from, whatever its size', async () => {
  render(<FileView scope={scope} path="logo.png" visible />);
  const image = (await screen.findByRole('img', { name: 'logo.png' })) as HTMLImageElement;
  expect(image.getAttribute('src')).toBe(
    'aura://app/workspace-media/00000000-0000-4000-8000-000000000000/logo.png',
  );
  expect(api.workspaceMediaUrl).toHaveBeenCalledWith({
    session_id: session,
    workspace_id: workspace,
    path: 'logo.png',
  });
  expect(api.readWorkspaceBinaryRange).not.toHaveBeenCalled();
  expect(api.readWorkspaceText).not.toHaveBeenCalled();
});

test('a video plays in its tab; one its player cannot play opens with its program', async () => {
  const user = userEvent.setup();
  const outside = vi.fn(async () => ({ ok: true, value: { completed: true, message: '' } }));
  Object.assign(window.aura.v1, { openWorkspaceFileOutside: outside });
  const { container } = render(<FileView scope={scope} path="baidu_video/clip.mp4" visible />);
  await vi.waitFor(() => expect(container.querySelector('video')).toBeTruthy());
  const video = container.querySelector('video')!;
  expect(video.getAttribute('src')).toMatch(/\/clip\.mp4$/u);
  expect(video.hasAttribute('controls')).toBe(true);
  video.dispatchEvent(new Event('error'));
  await user.click(await screen.findByRole('button', { name: '用系统程序打开' }));
  expect(outside).toHaveBeenCalledWith({
    session_id: session,
    workspace_id: workspace,
    path: 'baidu_video/clip.mp4',
    opener: 'default_application',
  });
});

test('a video fills the sidebar content and restores on its button or Escape', async () => {
  const user = userEvent.setup();
  // jsdom has the popover attribute's hidden style, but not Chromium's top-layer APIs.
  Object.defineProperty(HTMLElement.prototype, 'showPopover', {
    configurable: true,
    value() {
      this.style.display = 'flex';
    },
  });
  Object.defineProperty(HTMLElement.prototype, 'hidePopover', {
    configurable: true,
    value() {
      this.style.removeProperty('display');
    },
  });
  const matches = Element.prototype.matches;
  vi.spyOn(Element.prototype, 'matches').mockImplementation(function (this: Element, selector) {
    return selector === ':popover-open'
      ? this.hasAttribute('popover')
      : matches.call(this, selector);
  });
  const { container } = render(<FileView scope={scope} path="baidu_video/clip.mp4" visible />);
  const expand = await screen.findByRole('button', { name: '填满侧边栏' });
  expect(container.querySelector('video')?.getAttribute('controlslist')).toBe('nofullscreen');
  await user.click(expand);
  expect(container.querySelector('.file-view-media')?.hasAttribute('data-expanded')).toBe(true);
  await user.click(screen.getByRole('button', { name: '还原视频大小' }));
  expect(container.querySelector('.file-view-media')?.hasAttribute('data-expanded')).toBe(false);
  await user.click(expand);
  await user.keyboard('{Escape}');
  expect(container.querySelector('.file-view-media')?.hasAttribute('data-expanded')).toBe(false);
  delete (HTMLElement.prototype as Partial<HTMLElement>).showPopover;
  delete (HTMLElement.prototype as Partial<HTMLElement>).hidePopover;
});

// A bare name the model wrote that is not at the root, with several files of that name.
test('a bare name with several files of that name lists them, and one click opens it', async () => {
  const user = userEvent.setup();
  render(<FileView scope={scope} path="video.html" visible />);
  await user.click(await screen.findByRole('button', { name: 'baidu_video/video.html' }));
  expect(toolTabs.get(session).tabs).toEqual([
    expect.objectContaining({ kind: 'files', file: 'baidu_video/video.html' }),
  ]);
});

test('a file that is not text is named by its type and size', async () => {
  render(<FileView scope={scope} path="data.bin" visible />);
  expect(await screen.findByText('这个文件不是文本，暂不能预览。')).toBeTruthy();
  expect(screen.getByText('ELF executable · 2.0 KB')).toBeTruthy();
});

test('a large file reads in parts', async () => {
  const user = userEvent.setup();
  const many = Array.from({ length: 1200 }, (_, index) => `line ${index + 1}`);
  api.readWorkspaceText.mockImplementation((async (request: {
    range: { range_kind: string; start_line_index?: number; count?: number };
  }) => {
    if (request.range.range_kind === 'full')
      return { ok: false, error: { code: 'fs.range_required', message: 'range' } };
    const start = request.range.start_line_index ?? 0;
    return {
      ok: true,
      value: {
        text: many.slice(start, start + (request.range.count ?? 0)).join('\n'),
        total_lines: 1200,
      },
    };
  }) as never);
  render(<FileView scope={scope} path="big.log" visible />);
  await screen.findByText('line 500');
  expect(document.querySelectorAll('.file-line')).toHaveLength(500);
  await user.click(screen.getByRole('button', { name: '继续读取（还有 700 行）' }));
  await screen.findByText('line 1000');
  expect(document.querySelectorAll('.file-line')).toHaveLength(1000);
  expect(api.readWorkspaceText.mock.calls.at(-1)?.[0]).toMatchObject({
    range: { range_kind: 'lines', start_line_index: 500, count: 500 },
  });
});

test('a missing file says so and can be read again', async () => {
  const user = userEvent.setup();
  render(<FileView scope={scope} path="gone.txt" visible />);
  expect((await screen.findByRole('alert')).textContent).toContain('找不到这个文件');
  await user.click(screen.getByRole('button', { name: '重新读取' }));
  expect(api.readWorkspaceText).toHaveBeenCalledTimes(2);
});

test('the line asked for is marked and brought into view', async () => {
  const scrolled = vi.fn();
  Element.prototype.scrollIntoView = scrolled;
  render(<FileView scope={scope} path="src/main.rs" line={2} shown={1} visible />);
  await screen.findByText('fn main() {');
  const target = document.querySelector('.file-line[aria-current="true"]');
  expect(target?.querySelector('.file-line-number')?.textContent).toBe('2');
  expect(scrolled).toHaveBeenCalled();
});
