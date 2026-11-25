import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { graphView } from '../graph-fixtures.test-support';
import { GraphTeamSection } from './GraphTeamSection';
afterEach(cleanup);

test('the other parents A1 worktree menu is bound to its real parent session', async () => {
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  const entry = view.teams.find((team) => team.parent_session_id === 'ses_b')!;
  entry.team.members[0]!.worktree = { branch: 'aura/agent/fixture', state: 'active' };
  const open = vi.fn().mockResolvedValue({ ok: true, value: { completed: true } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { openAgentTeamWorktree: open } },
  });
  render(
    <GraphTeamSection
      entry={entry}
      profile={view.profiles[1]}
      currentSession="ses_a"
      chosen={undefined}
      onPick={vi.fn()}
      onEditProfile={vi.fn()}
    />,
  );
  fireEvent.contextMenu(screen.getByRole('button', { name: /检查\s*A1/u }));
  fireEvent.click(await screen.findByRole('menuitem', { name: '在文件管理器中查看工作区' }));
  await waitFor(() => expect(open).toHaveBeenCalledWith({ session_id: 'ses_b', agent: 'A1' }));
  expect(open).toHaveBeenCalledTimes(1);
});
