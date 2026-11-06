import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SessionModelSelectorProjection } from '@aura/contracts/generated';
import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests, updateComposerDraft } from '../agent-session/composer-drafts';
import type { FileActionScope } from '../files/file-menu';

const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;
const PROGRAMS = [
  { app_id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: '记事本' },
  { app_id: 'Microsoft.VisualStudioCode', name: 'Visual Studio Code' },
];

/** The first bytes of a PNG: the image is not decoded, only its address is checked. */
const PNG = 'iVBORw0KGgo=';
const catalog = vi.fn();
const icons = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  catalog.mockReset().mockResolvedValue({ ok: true, value: { enabled: true, apps: PROGRAMS } });
  // Notepad has an icon; Visual Studio Code has none.
  icons.mockReset().mockImplementation(({ app_ids }: { app_ids: string[] }) =>
    Promise.resolve({
      ok: true,
      value: {
        icons: app_ids
          .filter((id) => id === PROGRAMS[0]!.app_id)
          .map((app_id) => ({ app_id, png: PNG })),
      },
    }),
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        findWorkspacePaths: vi.fn().mockResolvedValue({
          ok: true,
          value: { catalog_revision: '1', matches: [] },
        }),
        submitAgentMessage: vi.fn().mockResolvedValue({ ok: true, value: {} }),
        computer: { catalog, icons },
      },
    },
  });
});
afterEach(cleanup);

function renderComposer() {
  const scope = { session: 'session', workspace: 'workspace', root: 'C:\\w', notify: vi.fn() };
  render(
    <ResizableComposer
      sessionId="session"
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={vi.fn()}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="workspace_read_write"
      promptInspector={undefined}
      fileScope={scope as unknown as FileActionScope}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}
const programs = () => screen.queryByRole('listbox', { name: '电脑上的程序' });

test('"@" lists the computer\'s programs while computer control is on; a pick writes its quoted name', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '用 @');
  await waitFor(() => expect(programs()).toBeTruthy());
  expect(
    within(programs()!)
      .getAllByRole('option')
      .map((option) => option.textContent),
  ).toEqual(['记事本', 'Visual Studio Code']);
  // Chinese names by the first letters of their pinyin.
  await user.keyboard('jsb');
  await waitFor(() =>
    expect(
      within(programs()!)
        .getAllByRole('option')
        .map((o) => o.textContent),
    ).toEqual(['记事本']),
  );
  await user.keyboard('{Enter}');
  expect(box.value).toBe('用 @"记事本" ');
  await user.type(box, '和 @visual');
  await waitFor(() => expect(programs()).toBeTruthy());
  await user.keyboard('{Enter}');
  expect(box.value).toBe('用 @"记事本" 和 @"Visual Studio Code" ');
});

test('while computer control is off the list has no programs', async () => {
  catalog.mockResolvedValue({ ok: true, value: { enabled: false, apps: [] } });
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(catalog).toHaveBeenCalled());
  expect(programs()).toBeNull();
});

const ICON = `data:image/png;base64,${PNG}`;
/** The block the composer draws in front of its words for a program, by the program's name. */
const block = (name: string) =>
  document.querySelector(`.composer-program-layer[data-layer='front'] [data-program='${name}']`);

test("the list shows each program's own icon, and the plain program icon for one without", async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '@');
  await waitFor(() => expect(programs()).toBeTruthy());
  const [notepad, code] = within(programs()!).getAllByRole('option');
  await waitFor(() => expect(notepad!.querySelector('img')?.getAttribute('src')).toBe(ICON));
  expect(code!.querySelector('img')).toBeNull();
  expect(code!.querySelector('svg')).not.toBeNull();
  // Asked for together, in the list's order.
  expect(icons.mock.calls).toEqual([[{ app_ids: PROGRAMS.map((program) => program.app_id) }]]);
});

test('a picked program shows in the composer as a block with its icon, and Backspace takes it whole', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '打开 @jsb');
  await waitFor(() => expect(programs()).toBeTruthy());
  await user.keyboard('{Enter}');
  expect(box.value).toBe('打开 @"记事本" ');
  await waitFor(() =>
    expect(block('记事本')?.querySelector('img')?.getAttribute('src')).toBe(ICON),
  );
  // Behind the words, the block is painted over the same characters the words have.
  const painted = document.querySelector(
    ".composer-program-layer[data-layer='back'] .composer-program-block",
  );
  expect(painted?.textContent).toBe('@"记事本"');
  await user.keyboard('{Backspace}');
  expect(box.value).toBe('打开 @"记事本"');
  await user.keyboard('{Backspace}');
  expect(box.value).toBe('打开 ');
  expect(box.selectionStart).toBe(3);
  expect(block('记事本')).toBeNull();
});

test('Delete just before a block takes it whole; a quoted name inside a word is no block', async () => {
  updateComposerDraft('session', { text: '@"记事本" 和 mail@"记事本" 写' });
  const box = renderComposer();
  await waitFor(() => expect(block('记事本')).not.toBeNull());
  expect(
    document.querySelectorAll(".composer-program-layer[data-layer='front'] [data-program]"),
  ).toHaveLength(1);
  box.focus();
  box.setSelectionRange(0, 0);
  fireEvent.keyDown(box, { key: 'Delete' });
  expect(box.value).toBe(' 和 mail@"记事本" 写');
  expect(box.selectionStart).toBe(0);
  expect(block('记事本')).toBeNull();
});

test('a kept draft that names a program in quotes shows the block once the programs are read', async () => {
  updateComposerDraft('session', { text: '用 @"记事本" 和 @"不是程序" 写' });
  renderComposer();
  await waitFor(() => expect(block('记事本')).not.toBeNull());
  expect(catalog).toHaveBeenCalledTimes(1);
  expect(block('不是程序')).toBeNull();
});
