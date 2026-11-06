import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SkillPreview, SkillView } from '@aura/contracts/generated';
import { SkillSettings } from './SkillSettings';

const WORKSPACE = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV' as NonNullable<SkillView['workspace_id']>;

const view = (relative: string, extra: Partial<SkillView> = {}): SkillView => ({
  id: `skl_${relative}`,
  source: 'agent_global',
  workspace_id: null,
  relative,
  path: `C:\\Users\\me\\.agents\\skills\\${relative}`,
  state: 'ready',
  enabled: true,
  name: relative,
  description: `${relative} 的说明`,
  license: null,
  compatibility: null,
  allowed_tools: [],
  fingerprint: `${relative}-fingerprint`,
  trusted_fingerprint: null,
  warnings: [],
  missing_references: [],
  problems: [],
  file_count: 2,
  bytes: 2048,
  ...extra,
});
const registry = (
  skills: SkillView[],
  revision = 4,
  extra: { workspace_trusted?: boolean; pinned_tokens?: number } = {},
) => ({
  ok: true,
  value: {
    revision,
    skills,
    roots: [
      { source: 'aura_store', path: 'C:\\Aura\\skills\\packages', exists: true },
      { source: 'agent_global', path: 'C:\\Users\\me\\.agents\\skills', exists: true },
      { source: 'claude_global', path: 'C:\\Users\\me\\.claude\\skills', exists: false },
    ],
    issue: null,
    workspace_trusted: extra.workspace_trusted ?? false,
    pinned_tokens: extra.pinned_tokens ?? 0,
    pinned_limit: 20000,
  },
});
const skills = () => [
  view('notes'),
  view('found', { state: 'disabled', enabled: false }),
  view('style', {
    source: 'workspace',
    workspace_id: WORKSPACE,
    state: 'not_trusted',
    path: 'C:\\Dev\\demo\\.agents\\skills\\style',
  }),
  view('edited', {
    source: 'workspace',
    workspace_id: WORKSPACE,
    state: 'content_changed',
    fingerprint: 'edited-new',
    trusted_fingerprint: 'edited-fingerprint',
    path: 'C:\\Dev\\demo\\.agents\\skills\\edited',
  }),
  view('pdf', { source: 'aura_store', path: 'C:\\Aura\\skills\\packages\\pdf' }),
  view('broken', {
    state: 'invalid',
    name: null,
    fingerprint: null,
    problems: ['缺少 description'],
  }),
];
const preview = (skill: SkillView): SkillPreview => ({
  skill,
  skill_md: `---\nname: ${skill.relative}\n---\n正文`,
  files: [
    { path: 'SKILL.md', bytes: 120 },
    { path: 'references/guide.md', bytes: 80 },
  ],
});

const api = {
  listSkills: vi.fn(),
  previewSkill: vi.fn(),
  untrustSkill: vi.fn(),
  trustWorkspaceSkills: vi.fn(),
  untrustWorkspaceSkills: vi.fn(),
  setSkillEnabled: vi.fn(),
  setSkillPinned: vi.fn(),
  importSkill: vi.fn(),
  removeSkill: vi.fn(),
  selectSkillDirectory: vi.fn(),
  selectSkillArchive: vi.fn(),
  openSkillDirectory: vi.fn(),
};
beforeEach(() => {
  for (const mock of Object.values(api)) mock.mockReset();
  api.listSkills.mockResolvedValue(registry(skills()));
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: api } });
});
afterEach(cleanup);

const rowOf = (name: string) =>
  screen.getByText(name, { selector: 'strong' }).closest('li') as HTMLElement;
/** The Skill's dialog, opened with its row's 配置. */
const details = async (name: string) => {
  await userEvent.click(within(rowOf(name)).getByRole('button', { name: `配置 ${name}` }));
  return screen.getByRole('dialog', { name });
};

test('lists Skills with source and state; every usable one has a switch and none asks for trust', async () => {
  render(<SkillSettings coreReady />);
  expect(await screen.findByText('notes', { selector: 'strong' })).toBeTruthy();
  expect(within(rowOf('notes')).getByText('可用')).toBeTruthy();
  expect(within(rowOf('notes')).getByText('我的 .agents')).toBeTruthy();
  expect(within(rowOf('found')).getByText('已停用')).toBeTruthy();
  expect(within(rowOf('style')).getByText('工作区未信任，模型看不到')).toBeTruthy();
  expect(within(rowOf('edited')).getByText('内容已变化，信任工作区后可用')).toBeTruthy();
  expect(within(rowOf('pdf')).getByText('已导入 Aura')).toBeTruthy();
  expect(within(rowOf('pdf')).getByText('可用')).toBeTruthy();
  expect(within(rowOf('broken')).getByText('缺少 description')).toBeTruthy();
  // No explanatory note on the page.
  expect(screen.queryByRole('note')).toBeNull();
  expect(within(rowOf('notes')).getByRole('button', { name: '停用' })).toBeTruthy();
  expect(within(rowOf('found')).getByRole('button', { name: '启用' })).toBeTruthy();
  expect(screen.queryByRole('button', { name: /信任并启用|查看并信任|重新信任/u })).toBeNull();
  // An unusable folder offers no preview, switch or pin.
  expect(within(rowOf('broken')).queryByRole('button', { name: /启用/u })).toBeNull();
  expect(
    within(await details('broken')).queryByRole('button', { name: /预览|启用|固定加载/u }),
  ).toBeNull();
  // A copy in Aura's own folder is removed; the user's own folders are deleted (see below).
  expect(within(await details('pdf')).getByRole('button', { name: '移除' })).toBeTruthy();
  expect(within(await details('notes')).queryByRole('button', { name: '移除' })).toBeNull();
  // Where Skills are read from, the user's .claude folder too.
  const roots = screen.getByRole('list', { name: 'Skill 位置' });
  expect(roots.textContent).toContain('我的 .claude：C:\\Users\\me\\.claude\\skills');
});

test('a row is the Skill in one line; its dialog holds the rest and closes', async () => {
  const user = userEvent.setup();
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  const row = rowOf('notes');
  expect(within(row).getByText('notes 的说明')).toBeTruthy();
  expect(within(row).queryByText('C:\\Users\\me\\.agents\\skills\\notes')).toBeNull();
  expect(
    within(row)
      .getAllByRole('button')
      .map((button) => button.textContent),
  ).toEqual(['停用', '配置']);
  const dialog = await details('notes');
  expect(within(dialog).getByText('C:\\Users\\me\\.agents\\skills\\notes')).toBeTruthy();
  expect(within(dialog).getByText('我的 .agents')).toBeTruthy();
  expect(within(dialog).getByText('可用')).toBeTruthy();
  await user.click(within(dialog).getByRole('button', { name: '关闭' }));
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('a dialog shows what is done in it, not what was done before to another Skill', async () => {
  const user = userEvent.setup();
  api.setSkillEnabled.mockResolvedValue(registry(skills(), 5));
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(within(rowOf('notes')).getByRole('button', { name: '停用' }));
  expect(await screen.findByText('已停用 notes')).toBeTruthy();
  const dialog = await details('pdf');
  expect(within(dialog).queryByText('已停用 notes')).toBeNull();
  expect(screen.queryByText('已停用 notes')).toBeNull();
});

test('pinning shows each pinned Skill’s size and the total, and a refusal shows Core’s sentence', async () => {
  const user = userEvent.setup();
  const pinned = [
    view('notes', { pinned: true, pin_tokens: 1200 }),
    view('grown', { pinned: true, pinned_left_out: true, pin_tokens: 30000 }),
    view('big'),
  ];
  api.listSkills.mockResolvedValue(registry(pinned, 4, { pinned_tokens: 1200 }));
  api.setSkillPinned
    .mockResolvedValueOnce({
      ok: false,
      error: {
        message:
          'skills.pin_limit: 固定加载的 Skill 合计不能超过 20000 token（已固定约 1200，这个约 30000），请先取消别的固定加载',
      },
    })
    .mockResolvedValueOnce(registry([view('notes'), pinned[1]!, view('big')], 5));
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  expect(screen.getByText('固定加载 约 1200 / 20000 token')).toBeTruthy();
  expect(within(rowOf('notes')).getByText('固定加载 约 1200 token')).toBeTruthy();
  expect(within(rowOf('grown')).getByText('超出上限，这次没有固定加载')).toBeTruthy();
  const big = await details('big');
  await user.click(within(big).getByRole('button', { name: '固定加载' }));
  expect(api.setSkillPinned).toHaveBeenCalledWith({
    id: 'skl_big',
    workspace_id: null,
    pinned: true,
    expected_revision: 4,
  });
  expect(await within(big).findByText(/合计不能超过 20000 token/u)).toBeTruthy();
  await waitFor(() => expect(api.listSkills).toHaveBeenCalledTimes(2));
  const notes = await details('notes');
  await user.click(within(notes).getByRole('button', { name: '取消固定加载' }));
  expect(api.setSkillPinned).toHaveBeenLastCalledWith({
    id: 'skl_notes',
    workspace_id: null,
    pinned: false,
    expected_revision: 4,
  });
  expect(await within(notes).findByText('已取消固定加载 notes')).toBeTruthy();
});

test('a workspace’s Skills are trusted once for the workspace, and the trust can be withdrawn', async () => {
  const user = userEvent.setup();
  api.trustWorkspaceSkills.mockResolvedValue(registry(skills(), 5, { workspace_trusted: true }));
  api.untrustWorkspaceSkills.mockResolvedValue(registry(skills(), 6));
  render(<SkillSettings coreReady workspaceId={WORKSPACE} />);
  await screen.findByText('notes', { selector: 'strong' });
  expect(api.listSkills).toHaveBeenCalledWith({ workspace_id: WORKSPACE });
  await user.click(screen.getByRole('button', { name: '信任当前工作区的 Skill' }));
  expect(api.trustWorkspaceSkills).toHaveBeenCalledWith({
    workspace_id: WORKSPACE,
    expected_revision: 4,
  });
  expect(await screen.findByText('已信任当前工作区的 Skill')).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '撤销对当前工作区的信任' }));
  expect(api.untrustWorkspaceSkills).toHaveBeenCalledWith({
    workspace_id: WORKSPACE,
    expected_revision: 5,
  });
  expect(await screen.findByText('已撤销对当前工作区的信任')).toBeTruthy();
});

test('without a workspace there is nothing to trust', async () => {
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  expect(screen.queryByRole('button', { name: /当前工作区/u })).toBeNull();
});

test('importing goes through Main’s folder dialog and a cancel changes nothing', async () => {
  const user = userEvent.setup();
  api.selectSkillDirectory.mockResolvedValueOnce({ ok: true, value: { path: null } });
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '导入文件夹…' }));
  expect(api.importSkill).not.toHaveBeenCalled();
  api.selectSkillDirectory.mockResolvedValueOnce({ ok: true, value: { path: 'D:\\skills\\pdf' } });
  api.importSkill.mockResolvedValue(registry(skills(), 5));
  await user.click(screen.getByRole('button', { name: '导入文件夹…' }));
  expect(api.importSkill).toHaveBeenCalledWith({ path: 'D:\\skills\\pdf', expected_revision: 4 });
  expect(await screen.findByText('已导入，可以使用')).toBeTruthy();
});

test('a ZIP file is imported through Main’s file dialog like a folder', async () => {
  const user = userEvent.setup();
  api.selectSkillArchive.mockResolvedValueOnce({ ok: true, value: { path: null } });
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(screen.getByRole('button', { name: '导入 ZIP…' }));
  expect(api.importSkill).not.toHaveBeenCalled();
  expect(api.selectSkillDirectory).not.toHaveBeenCalled();
  api.selectSkillArchive.mockResolvedValueOnce({ ok: true, value: { path: 'D:/skills/pdf.zip' } });
  api.importSkill.mockResolvedValue(registry(skills(), 5));
  await user.click(screen.getByRole('button', { name: '导入 ZIP…' }));
  expect(api.importSkill).toHaveBeenCalledWith({ path: 'D:/skills/pdf.zip', expected_revision: 4 });
  expect(await screen.findByText('已导入，可以使用')).toBeTruthy();
});

test('switching, withdrawing an earlier trust and removing name the revision; removal asks first', async () => {
  const user = userEvent.setup();
  api.setSkillEnabled.mockResolvedValue(registry(skills(), 5));
  api.untrustSkill.mockResolvedValue(registry(skills(), 6));
  api.removeSkill.mockResolvedValue(
    registry(
      skills().filter((s) => s.relative !== 'pdf'),
      7,
    ),
  );
  render(<SkillSettings coreReady workspaceId={WORKSPACE} />);
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(within(rowOf('notes')).getByRole('button', { name: '停用' }));
  expect(api.setSkillEnabled).toHaveBeenCalledWith({
    id: 'skl_notes',
    workspace_id: null,
    expected_revision: 4,
    enabled: false,
  });
  // Only a workspace Skill trusted alone before keeps a way to withdraw that trust.
  expect(within(await details('notes')).queryByRole('button', { name: '撤销信任' })).toBeNull();
  await user.click(within(await details('edited')).getByRole('button', { name: '撤销信任' }));
  expect(api.untrustSkill).toHaveBeenCalledWith({
    id: 'skl_edited',
    workspace_id: WORKSPACE,
    expected_revision: 5,
  });
  await user.click(within(await details('pdf')).getByRole('button', { name: '移除' }));
  const confirm = screen.getByRole('alertdialog', { name: '移除 pdf' });
  expect(confirm.textContent).toContain('原文件夹不受影响');
  expect(api.removeSkill).not.toHaveBeenCalled();
  await user.click(within(confirm).getByRole('button', { name: '移除' }));
  expect(api.removeSkill).toHaveBeenCalledWith({
    id: 'skl_pdf',
    workspace_id: null,
    expected_revision: 6,
  });
  await waitFor(() => expect(screen.queryByText('pdf', { selector: 'strong' })).toBeNull());
  // The removed Skill's dialog closes with it; the outcome is said on the page.
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.getByRole('status').textContent).toBe('已移除 pdf');
});

test('a removed Skill’s dialog stays closed when the list has a Skill of that id again', async () => {
  const user = userEvent.setup();
  // Imported again at once: the list after the removal still has it.
  api.removeSkill.mockResolvedValue(registry(skills(), 5));
  render(<SkillSettings coreReady />);
  await screen.findByText('pdf', { selector: 'strong' });
  await user.click(within(await details('pdf')).getByRole('button', { name: '移除' }));
  const confirm = screen.getByRole('alertdialog', { name: '移除 pdf' });
  await user.click(within(confirm).getByRole('button', { name: '移除' }));
  expect(await screen.findByText('已移除 pdf')).toBeTruthy();
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.getByText('pdf', { selector: 'strong' })).toBeTruthy();
});

test("a refusal shows Core's sentence and reads the list again", async () => {
  const user = userEvent.setup();
  api.setSkillEnabled.mockResolvedValue({
    ok: false,
    error: { message: 'skills.revision_changed: Skill 设置已被修改，请刷新后再操作' },
  });
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(within(rowOf('notes')).getByRole('button', { name: '停用' }));
  expect(await screen.findByText('Skill 设置已被修改，请刷新后再操作')).toBeTruthy();
  await waitFor(() => expect(api.listSkills).toHaveBeenCalledTimes(2));
});

test('a Skill folder is opened by Main from its id, a broken one too', async () => {
  const user = userEvent.setup();
  api.openSkillDirectory
    .mockResolvedValueOnce({ ok: true, value: { completed: true, message: '已打开 Skill 文件夹' } })
    .mockResolvedValueOnce({
      ok: true,
      value: { completed: false, message: '这个 Skill 的文件夹已不存在，请刷新后再试。' },
    });
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  const notes = await details('notes');
  await user.click(within(notes).getByRole('button', { name: '打开文件夹' }));
  expect(api.openSkillDirectory).toHaveBeenCalledWith({ id: 'skl_notes', workspace_id: null });
  expect(await within(notes).findByText('已打开 Skill 文件夹')).toBeTruthy();
  // A folder that is not a valid Skill can still be opened to fix it; a refusal re-reads the list.
  const broken = await details('broken');
  await user.click(within(broken).getByRole('button', { name: '打开文件夹' }));
  expect(
    await within(broken).findByText('这个 Skill 的文件夹已不存在，请刷新后再试。'),
  ).toBeTruthy();
  await waitFor(() => expect(api.listSkills).toHaveBeenCalledTimes(2));
});

test('a plugin’s Skill shows its plugin, is switched and pinned here, and is removed with it', async () => {
  api.listSkills.mockResolvedValue(
    registry([
      view('guide', {
        source: 'plugin',
        plugin: 'kit',
        path: 'C:\\Aura\\plugins\\packages\\kit\\skills\\guide',
      }),
    ]),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('guide', { selector: 'strong' });
  expect(within(rowOf('guide')).getByText('插件 kit')).toBeTruthy();
  const shown = await details('guide');
  for (const name of ['撤销信任', '移除', '删除'])
    expect(within(shown).queryByRole('button', { name })).toBeNull();
  for (const name of ['预览', '停用', '固定加载'])
    expect(within(shown).getByRole('button', { name })).toBeTruthy();
});

// The button that opened a preview closes it; the preview has no close
// button of its own, far from the one that opened it.
test('the preview button also closes the preview it opened', async () => {
  const user = userEvent.setup();
  api.previewSkill.mockImplementation(({ id }: { id: string }) =>
    Promise.resolve({
      ok: true,
      value: preview(skills().find((skill) => skill.id === id)!),
    }),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  const dialog = await details('notes');
  await user.click(within(dialog).getByRole('button', { name: '预览' }));
  const panel = await within(dialog).findByRole('region', { name: '预览 Skill notes' });
  expect(api.previewSkill).toHaveBeenCalledWith({ id: 'skl_notes', workspace_id: null });
  expect(panel.textContent).toContain('正文');
  expect(panel.textContent).toContain('references/guide.md');
  expect(within(panel).queryByRole('button', { name: /信任|关闭/u })).toBeNull();
  const toggle = within(dialog).getByRole('button', { name: '收起' });
  expect(toggle.getAttribute('aria-expanded')).toBe('true');
  await user.click(toggle);
  expect(screen.queryByRole('region', { name: '预览 Skill notes' })).toBeNull();
  expect(within(dialog).getByRole('button', { name: '预览' })).toBeTruthy();
});

test('a Skill in the user’s own folder is deleted to the Recycle Bin after a confirmation', async () => {
  const user = userEvent.setup();
  api.removeSkill.mockResolvedValue(
    registry(
      skills().filter((s) => s.relative !== 'notes'),
      5,
    ),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  // An imported copy is removed, not deleted.
  expect(within(await details('pdf')).queryByRole('button', { name: '删除' })).toBeNull();
  const dialog = await details('notes');
  await user.click(within(dialog).getByRole('button', { name: '删除' }));
  const confirm = screen.getByRole('alertdialog', { name: '删除 notes' });
  expect(confirm.textContent).toContain(
    '删除后，这个 Skill 的文件夹会移到回收站：C:\\Users\\me\\.agents\\skills\\notes',
  );
  expect(confirm.textContent).toContain('其他使用 .agents 的工具也会看不到它。');
  await user.click(within(confirm).getByRole('button', { name: '取消' }));
  expect(api.removeSkill).not.toHaveBeenCalled();
  await user.click(within(dialog).getByRole('button', { name: '删除' }));
  await user.click(
    within(screen.getByRole('alertdialog', { name: '删除 notes' })).getByRole('button', {
      name: '删除',
    }),
  );
  expect(api.removeSkill).toHaveBeenCalledWith({
    id: 'skl_notes',
    workspace_id: null,
    expected_revision: 4,
  });
  expect(await screen.findByText('已删除 notes')).toBeTruthy();
});

test('a workspace Skill names only its folder, and a plugin’s Skill has no delete', async () => {
  const user = userEvent.setup();
  api.listSkills.mockResolvedValue(
    registry([
      view('local', {
        source: 'workspace',
        workspace_id: WORKSPACE,
        path: 'C:\\Dev\\demo\\.agents\\skills\\local',
      }),
      view('bundled', { source: 'plugin', plugin: 'tools' }),
    ]),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('local', { selector: 'strong' });
  expect(within(await details('bundled')).queryByRole('button', { name: '删除' })).toBeNull();
  await user.click(within(await details('local')).getByRole('button', { name: '删除' }));
  const confirm = screen.getByRole('alertdialog', { name: '删除 local' });
  expect(confirm.textContent).toContain('C:\\Dev\\demo\\.agents\\skills\\local');
  expect(confirm.textContent).not.toContain('其他使用 .agents 的工具');
});

test('a Skill that comes with Aura says so, is switched and pinned here, and has no delete', async () => {
  api.listSkills.mockResolvedValue(
    registry([
      view('write-aura-plugin', {
        source: 'aura_builtin',
        path: 'C:\\Aura\\skills\\builtin\\write-aura-plugin',
      }),
    ]),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('write-aura-plugin', { selector: 'strong' });
  expect(within(rowOf('write-aura-plugin')).getByText('Aura 自带')).toBeTruthy();
  const shown = await details('write-aura-plugin');
  expect(within(shown).getByRole('button', { name: '停用' })).toBeTruthy();
  expect(within(shown).getByRole('button', { name: '固定加载' })).toBeTruthy();
  expect(within(shown).queryByRole('button', { name: '删除' })).toBeNull();
  expect(within(shown).queryByRole('button', { name: '移除' })).toBeNull();
});

test('a delete Core refuses shows its reason and keeps the Skill', async () => {
  const user = userEvent.setup();
  api.removeSkill.mockResolvedValue({
    ok: false,
    error: {
      code: 'skills.not_recyclable',
      message:
        'skills.not_recyclable: 回收站放不下这个文件夹（或它所在的位置没有回收站），没有删除，它还在原处；请在资源管理器中处理。',
    },
  });
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  const dialog = await details('notes');
  await user.click(within(dialog).getByRole('button', { name: '删除' }));
  await user.click(
    within(screen.getByRole('alertdialog', { name: '删除 notes' })).getByRole('button', {
      name: '删除',
    }),
  );
  expect(await within(dialog).findByText(/回收站放不下这个文件夹/u)).toBeTruthy();
  expect(screen.getByText('notes', { selector: 'strong' })).toBeTruthy();
});
