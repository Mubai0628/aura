import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SkillPreview, SkillView } from '@aura/contracts/generated';
import { SkillSettings } from './SkillSettings';

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
  trusted_fingerprint: `${relative}-fingerprint`,
  warnings: [],
  missing_references: [],
  problems: [],
  file_count: 2,
  bytes: 2048,
  ...extra,
});
const registry = (skills: SkillView[], revision = 4) => ({
  ok: true,
  value: {
    revision,
    skills,
    roots: [
      { source: 'aura_store', path: 'C:\\Aura\\skills\\packages', exists: true },
      { source: 'agent_global', path: 'C:\\Users\\me\\.agents\\skills', exists: true },
    ],
    issue: null,
  },
});
const skills = () => [
  view('notes'),
  view('found', { state: 'disabled', enabled: false, trusted_fingerprint: null }),
  view('edited', { state: 'content_changed', fingerprint: 'edited-new' }),
  view('pdf', { source: 'aura_store', state: 'not_trusted', trusted_fingerprint: null }),
  view('broken', {
    state: 'invalid',
    name: null,
    fingerprint: null,
    trusted_fingerprint: null,
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
  trustSkill: vi.fn(),
  untrustSkill: vi.fn(),
  setSkillEnabled: vi.fn(),
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

test('lists Skills with source and state; only a trusted one has a switch', async () => {
  render(<SkillSettings coreReady />);
  expect(await screen.findByText('notes', { selector: 'strong' })).toBeTruthy();
  expect(within(rowOf('notes')).getByText('可用')).toBeTruthy();
  expect(within(rowOf('notes')).getByText('我的 .agents')).toBeTruthy();
  expect(within(rowOf('found')).getByText('未启用，未信任')).toBeTruthy();
  expect(within(rowOf('edited')).getByText('内容已变化，需要重新信任')).toBeTruthy();
  expect(within(rowOf('pdf')).getByText('已导入 Aura')).toBeTruthy();
  expect(within(rowOf('broken')).getByText('缺少 description')).toBeTruthy();
  // No explanatory note on the page.
  expect(screen.queryByRole('note')).toBeNull();
  expect(within(rowOf('notes')).getByRole('button', { name: '停用' })).toBeTruthy();
  expect(within(rowOf('found')).queryByRole('button', { name: '启用' })).toBeNull();
  expect(within(rowOf('found')).getByRole('button', { name: '查看并信任' })).toBeTruthy();
  expect(within(rowOf('edited')).getByRole('button', { name: '查看变化并重新信任' })).toBeTruthy();
  // An unusable folder offers no preview or trust; there is nothing it could be trusted at.
  expect(within(rowOf('broken')).queryByRole('button', { name: /查看|预览|启用/u })).toBeNull();
  // Only a copy in Aura's own folder can be removed; the user's folders are switched off.
  expect(within(rowOf('pdf')).getByRole('button', { name: '移除' })).toBeTruthy();
  expect(within(rowOf('notes')).queryByRole('button', { name: '移除' })).toBeNull();
});

test('trust previews SKILL.md and the files, then trusts exactly the shown fingerprint', async () => {
  const user = userEvent.setup();
  const found = skills()[1]!;
  api.previewSkill.mockResolvedValue({ ok: true, value: preview(found) });
  api.trustSkill.mockResolvedValue(
    registry([view('notes'), view('found', { trusted_fingerprint: 'found-fingerprint' })], 5),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('found', { selector: 'strong' });
  await user.click(within(rowOf('found')).getByRole('button', { name: '查看并信任' }));
  const panel = await screen.findByRole('region', { name: '预览 Skill found' });
  expect(api.previewSkill).toHaveBeenCalledWith({ id: 'skl_found', workspace_id: null });
  expect(panel.textContent).toContain('正文');
  expect(panel.textContent).toContain('references/guide.md');
  await user.click(within(panel).getByRole('button', { name: '信任并启用' }));
  expect(api.trustSkill).toHaveBeenCalledWith({
    id: 'skl_found',
    workspace_id: null,
    expected_revision: 4,
    expected_fingerprint: 'found-fingerprint',
  });
  expect(await screen.findByText('已信任并启用 found')).toBeTruthy();
  expect(screen.queryByRole('region', { name: '预览 Skill found' })).toBeNull();
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
  expect(await screen.findByText('已导入；查看并信任后模型才能使用')).toBeTruthy();
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
  expect(await screen.findByText('已导入；查看并信任后模型才能使用')).toBeTruthy();
});

test('switching, untrusting and removing name the revision; removal asks first', async () => {
  const user = userEvent.setup();
  api.setSkillEnabled.mockResolvedValue(registry(skills(), 5));
  api.untrustSkill.mockResolvedValue(registry(skills(), 6));
  api.removeSkill.mockResolvedValue(
    registry(
      skills().filter((s) => s.relative !== 'pdf'),
      7,
    ),
  );
  render(<SkillSettings coreReady />);
  await screen.findByText('notes', { selector: 'strong' });
  await user.click(within(rowOf('notes')).getByRole('button', { name: '停用' }));
  expect(api.setSkillEnabled).toHaveBeenCalledWith({
    id: 'skl_notes',
    workspace_id: null,
    expected_revision: 4,
    enabled: false,
  });
  await user.click(within(rowOf('notes')).getByRole('button', { name: '撤销信任' }));
  expect(api.untrustSkill).toHaveBeenCalledWith({
    id: 'skl_notes',
    workspace_id: null,
    expected_revision: 5,
  });
  await user.click(within(rowOf('pdf')).getByRole('button', { name: '移除' }));
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
  await user.click(within(rowOf('notes')).getByRole('button', { name: '打开文件夹' }));
  expect(api.openSkillDirectory).toHaveBeenCalledWith({ id: 'skl_notes', workspace_id: null });
  expect(await screen.findByText('已打开 Skill 文件夹')).toBeTruthy();
  // A folder that is not a valid Skill can still be opened to fix it; a refusal re-reads the list.
  await user.click(within(rowOf('broken')).getByRole('button', { name: '打开文件夹' }));
  expect(await screen.findByText('这个 Skill 的文件夹已不存在，请刷新后再试。')).toBeTruthy();
  await waitFor(() => expect(api.listSkills).toHaveBeenCalledTimes(2));
});

test('a workspace adds its own Skills to the listing', async () => {
  render(<SkillSettings coreReady workspaceId={'wsp_1' as never} />);
  await screen.findByText('notes', { selector: 'strong' });
  expect(api.listSkills).toHaveBeenCalledWith({ workspace_id: 'wsp_1' });
});

test('a plugin’s Skill shows its plugin and is trusted, switched and removed with it', async () => {
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
  const row = screen.getByText('guide', { selector: 'strong' }).closest('li') as HTMLElement;
  expect(within(row).getByText('插件 kit')).toBeTruthy();
  for (const name of ['停用', '撤销信任', '移除', '查看并信任'])
    expect(within(row).queryByRole('button', { name })).toBeNull();
  expect(within(row).getByRole('button', { name: '预览' })).toBeTruthy();
  expect(within(row).getByText(/请在 设置 → 插件 里管理/u)).toBeTruthy();
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
  await user.click(within(rowOf('notes')).getByRole('button', { name: '预览' }));
  const panel = await screen.findByRole('region', { name: '预览 Skill notes' });
  expect(within(panel).queryByRole('button', { name: '关闭' })).toBeNull();
  const toggle = within(rowOf('notes')).getByRole('button', { name: '收起' });
  expect(toggle.getAttribute('aria-expanded')).toBe('true');
  await user.click(toggle);
  expect(screen.queryByRole('region', { name: '预览 Skill notes' })).toBeNull();
  expect(within(rowOf('notes')).getByRole('button', { name: '预览' })).toBeTruthy();
  // A review before trusting closes the same way; its trust button stays in the review.
  await user.click(within(rowOf('found')).getByRole('button', { name: '查看并信任' }));
  const review = await screen.findByRole('region', { name: '预览 Skill found' });
  expect(within(review).getByRole('button', { name: '信任并启用' })).toBeTruthy();
  expect(within(review).queryByRole('button', { name: '关闭' })).toBeNull();
  await user.click(within(rowOf('found')).getByRole('button', { name: '收起' }));
  expect(screen.queryByRole('region', { name: '预览 Skill found' })).toBeNull();
});
