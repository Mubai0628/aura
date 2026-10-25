import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { FileLinkContext } from './file-links';
import { root, session, workspace } from './files-fixture';
import { MarkdownBody } from '../markdown-body';
import { resetToolTabsForTests, toolTabs } from '../right-workspace/tool-tabs';

// GPT models name the files
// they wrote as Markdown links to local paths. The renderer's link guard took those for unsafe links
// and showed "name [blocked]"; DeepSeek writes the path as inline code, which was already a file
// link. A link to a local file is now that same file link.

const scope = { session, workspace, root, notify: vi.fn() };
beforeEach(() => resetToolTabsForTests());
afterEach(cleanup);

const active = () => {
  const state = toolTabs.get(session);
  return state.tabs.find((tab) => tab.id === state.active);
};

const inConversation = (text: string) =>
  render(
    <FileLinkContext.Provider value={scope}>
      <MarkdownBody text={text} />
    </FileLinkContext.Provider>,
  );

test.each([
  ['a relative path', '已写好：[subway-runner.html](subway-runner.html)。'],
  ['an absolute path', '已写好：[subway-runner.html](C:\\Dev\\Demo\\subway-runner.html)。'],
  [
    'an absolute path with slashes',
    '已写好：[subway-runner.html](C:/Dev/Demo/subway-runner.html)。',
  ],
  ['a file address', '已写好：[subway-runner.html](file:///C:/Dev/Demo/subway-runner.html)。'],
])('a Markdown link to a workspace file by %s is the file link', async (_, text) => {
  const user = userEvent.setup();
  inConversation(text);
  expect(document.body.textContent).toBe('已写好：subway-runner.html。');
  expect(screen.getByText('subway-runner.html').tagName).toBe('CODE');
  await user.click(screen.getByRole('button', { name: '打开 subway-runner.html' }));
  expect(active()).toMatchObject({ kind: 'files', file: 'subway-runner.html' });
});

test('a link whose words are not the file keeps them and names the file after them', async () => {
  const user = userEvent.setup();
  inConversation('[入口在这里](src/main.rs#L3)');
  expect(document.body.textContent).toBe('入口在这里（src/main.rs#L3）');
  await user.click(screen.getByRole('button', { name: '打开 src/main.rs#L3' }));
  expect(active()).toMatchObject({ kind: 'files', file: 'src/main.rs', line: 3 });
});

test('a file outside the workspace is shown as its path, not as a blocked link', () => {
  inConversation('[hosts](C:\\Windows\\System32\\drivers\\etc\\hosts)');
  expect(document.body.textContent).toBe('C:/Windows/System32/drivers/etc/hosts');
  expect(screen.queryByRole('button')).toBeNull();
});

test('outside a conversation a local link is its path, never blocked', () => {
  render(<MarkdownBody text="[a.html](a.html)" />);
  expect(document.body.textContent).toBe('a.html');
  expect(screen.queryByRole('button')).toBeNull();
});

test('a web link is still a web link and a link in code stays code', () => {
  inConversation('[文档](https://example.com/a) 与 `[x](y.html)`');
  expect(document.body.textContent).toBe('文档（https://example.com/a） 与 [x](y.html)');
});
