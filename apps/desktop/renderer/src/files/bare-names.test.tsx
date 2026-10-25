import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './file-links';
import { root, session, workspace } from './files-fixture';
import { MarkdownBody } from '../markdown-body';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// The model named files by their bare name
// (`fengjing_01.jpg`), which was looked for at the workspace root although they were in a folder.
// A bare name not at the root opens the workspace file of that name.

const notify = vi.fn();
const scope = { session, workspace, root, notify };
const atRoot = new Set<string>();
const named: Record<string, string[]> = {};
const stat = vi.fn(async (request: { target: { path: string } }) =>
  atRoot.has(request.target.path)
    ? { ok: true, value: { path: request.target.path, kind: 'file' } }
    : { ok: false, error: { code: 'file.not_found', message: 'not found' } },
);
const glob = vi.fn(async (request: { pattern: string }) => ({
  ok: true,
  value: { paths: named[request.pattern] ?? [], catalog_revision: '1' },
}));
const openOutside = vi.fn();

beforeEach(() => {
  resetToolTabsForTests();
  atRoot.clear();
  for (const key of Object.keys(named)) delete named[key];
  stat.mockClear();
  glob.mockClear();
  openOutside.mockReset().mockResolvedValue({ ok: true, value: { completed: true, message: '' } });
  vi.stubGlobal('aura', {
    v1: {
      statWorkspaceFile: stat,
      globWorkspacePaths: glob,
      openWorkspaceFileOutside: openOutside,
    },
  });
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const message = (text: string) =>
  render(
    <FileLinkContext.Provider value={scope}>
      <MarkdownBody text={text} />
    </FileLinkContext.Provider>,
  );
const opened = () =>
  toolTabs.get(session).tabs.map((tab) => (tab.kind === 'files' ? tab.file : tab.kind));

test('a bare name not at the root opens the only workspace file of that name', async () => {
  const user = userEvent.setup();
  named['**/fengjing_01.jpg'] = ['baidu_landscape/fengjing_01.jpg'];
  message('| 文件 |\n|---|\n| `fengjing_01.jpg` |');
  await user.click(screen.getByRole('button', { name: '打开 fengjing_01.jpg' }));
  await vi.waitFor(() => expect(opened()).toEqual(['baidu_landscape/fengjing_01.jpg']));
});

test('a bare name at the root opens there, with no search', async () => {
  const user = userEvent.setup();
  atRoot.add('video.html');
  message('或打开 `video.html`');
  await user.click(screen.getByRole('button', { name: '打开 video.html' }));
  await vi.waitFor(() => expect(opened()).toEqual(['video.html']));
  expect(glob).not.toHaveBeenCalled();
});

test('a path with its folder opens as written', async () => {
  const user = userEvent.setup();
  message('`baidu_video/video.html`');
  await user.click(screen.getByRole('button', { name: '打开 baidu_video/video.html' }));
  await vi.waitFor(() => expect(opened()).toEqual(['baidu_video/video.html']));
  expect(stat).not.toHaveBeenCalled();
});

// A video plays in the sidebar; what its
// player cannot play still opens with its program.
test('a video opens in the sidebar; one its player cannot play opens outside', async () => {
  const user = userEvent.setup();
  message('`baidu_video/fengjing_video.mp4` 和 `clips/raw.mkv`');
  await user.click(screen.getByRole('button', { name: '打开 baidu_video/fengjing_video.mp4' }));
  await vi.waitFor(() => expect(opened()).toEqual(['baidu_video/fengjing_video.mp4']));
  await user.click(screen.getByRole('button', { name: '打开 clips/raw.mkv' }));
  expect(openOutside).toHaveBeenCalledWith(
    expect.objectContaining({ path: 'clips/raw.mkv', opener: 'default_application' }),
  );
});
