import { cleanup, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SessionModelSelectorProjection, SkillView } from '@aura/contracts/generated';
import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests } from '../agent-session/composer-drafts';
import { ChosenSkillsLine } from './SkillChoice';

const skill = (name: string, state: SkillView['state']): SkillView => ({
  id: `skl_${name}`,
  source: 'aura_store',
  workspace_id: null,
  relative: name,
  path: `C:\\Aura\\skills\\packages\\${name}`,
  state,
  enabled: true,
  name,
  description: `${name} 的说明`,
  license: null,
  compatibility: null,
  allowed_tools: [],
  fingerprint: `${name}-fingerprint`,
  trusted_fingerprint: state === 'ready' ? `${name}-fingerprint` : null,
  warnings: [],
  missing_references: [],
  problems: [],
  file_count: 1,
  bytes: 10,
});
const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;

const submit = vi.fn();
const listSkills = vi.fn();
beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  listSkills.mockReset().mockResolvedValue({
    ok: true,
    value: {
      revision: 1,
      skills: [skill('notes', 'ready'), skill('draft', 'not_trusted')],
      roots: [],
      issue: null,
    },
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        listSkills,
        submitAgentMessage: submit,
      },
    },
  });
});
afterEach(cleanup);

function renderComposer(active = false) {
  render(
    <ResizableComposer
      sessionId="session"
      activeTurn={active ? 'turn' : undefined}
      activeRunId={active ? 'run' : undefined}
      workspaceId="workspace"
      queuePending={false}
      onChanged={vi.fn()}
      onStop={vi.fn()}
      demoMode={false}
      modelSelector={selector}
      onOpenModelSettings={vi.fn()}
      onOpenPromptInspector={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      onOpenSettings={vi.fn()}
      onOpenPanel={vi.fn()}
      onSelectModel={vi.fn()}
      onSetPermissionMode={vi.fn()}
      permissionMode="read_only"
      promptInspector={undefined}
    />,
  );
  return screen.getByRole('textbox', { name: '输入消息' }) as HTMLTextAreaElement;
}

async function choose(user: ReturnType<typeof userEvent.setup>, name: string) {
  await user.click(screen.getByRole('button', { name: '添加' }));
  await user.click(screen.getByRole('menuitem', { name: /Skill/u }));
  const picker = await screen.findByRole('region', { name: '选择 Skill' });
  await user.click(await within(picker).findByRole('button', { name: new RegExp(name, 'u') }));
  return picker;
}

test('a trusted Skill is chosen from + and sent with the message that starts the next turn', async () => {
  const user = userEvent.setup();
  const box = renderComposer();
  await user.type(box, '按笔记格式回答');
  const picker = await choose(user, 'notes');
  expect(listSkills).toHaveBeenCalledWith({ workspace_id: 'workspace' });
  // Only a usable Skill is offered: one that is not trusted is not listed at all.
  expect(within(picker).queryByRole('button', { name: /draft/u })).toBeNull();
  expect(within(picker).getByRole('button', { name: /notes/u })).toHaveProperty('disabled', true);
  const chips = screen.getByRole('group', { name: '本轮使用的 Skill' });
  expect(chips.textContent).toContain('Skill：notes');

  await user.click(box);
  await user.keyboard('{Enter}');
  expect(submit).toHaveBeenCalledWith({
    session_id: 'session',
    text: '按笔记格式回答',
    destination: { lane: 'next_turn' },
    skill_ids: ['skl_notes'],
  });
  await vi.waitFor(() =>
    expect(screen.queryByRole('group', { name: '本轮使用的 Skill' })).toBeNull(),
  );
});

// While a task runs, a message waits in the queue; a chosen Skill goes with it and is used
// when the next turn starts (the queue does not append it to the running task).
test('with a task running, a message and its chosen Skill wait in the queue', async () => {
  const user = userEvent.setup();
  const box = renderComposer(true);
  await user.type(box, '再补充一点');
  await choose(user, 'notes');
  await user.click(screen.getByRole('button', { name: '加入下一轮队列' }));
  expect(submit).toHaveBeenCalledWith({
    session_id: 'session',
    text: '再补充一点',
    destination: { lane: 'next_turn' },
    skill_ids: ['skl_notes'],
  });
});

test('the conversation and the queue name each chosen Skill once', () => {
  render(<ChosenSkillsLine skills={[{ name: 'notes' }, { name: 'pdf' }, { name: 'notes' }]} />);
  expect(screen.getByText('已选 Skill：notes、pdf')).toBeTruthy();
  cleanup();
  const { container } = render(<ChosenSkillsLine skills={[]} />);
  expect(container.textContent).toBe('');
});
