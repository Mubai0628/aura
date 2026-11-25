import type { AgentTeamMemberView, AgentTeamView } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { AgentTeamCanvas } from './AgentTeamCanvas';
import { FLOW_MS } from './flows';
import { resetPlacesForTests } from './places';

// The 多 Agent canvas: a card per agent with its facts, a line from the main agent to each
// sub-agent, what passes along a line lit for a moment, picking, zoom and moving cards.

beforeEach(() => {
  vi.stubGlobal(
    'PointerEvent',
    class extends MouseEvent {
      readonly pointerId = 1;
    },
  );
  Element.prototype.setPointerCapture = vi.fn();
});
afterEach(() => {
  cleanup();
  resetPlacesForTests();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  for (const size of ['clientWidth', 'clientHeight'])
    if (Object.hasOwn(HTMLElement.prototype, size))
      delete (HTMLElement.prototype as unknown as Record<string, unknown>)[size];
});

/** A board `initial` pixels wide; the function returned resizes it as the panel would. */
function sizedBoard(initial: number) {
  let width = initial;
  let resized: (() => void) | undefined;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resized = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  const sized = (pixels: () => number) => ({
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains('agent-canvas') ? pixels() : 0;
    },
  });
  Object.defineProperty(
    HTMLElement.prototype,
    'clientWidth',
    sized(() => width),
  );
  Object.defineProperty(
    HTMLElement.prototype,
    'clientHeight',
    sized(() => 700),
  );
  return (next: number) => {
    width = next;
    act(() => resized?.());
  };
}

const member = (agent: string, name: string, more: Partial<AgentTeamMemberView> = {}) =>
  ({
    agent,
    name,
    session_id: `ses_${agent}`,
    model_profile_id: 'mpr_flash',
    model: 'DeepSeek V4.1 Flash',
    state: 'running',
    working: true,
    approvals: [],
    tokens: 0,
    ...more,
  }) as AgentTeamMemberView;

const team = (...members: AgentTeamMemberView[]) =>
  ({
    lead: {
      model_profile_id: 'mpr_sol',
      model: 'GPT 6.1 Sol',
      working: true,
      tokens: 12000,
      tool: 'agents.wait',
    },
    members,
  }) as AgentTeamView;

function shown(view: AgentTeamView, chosen?: string) {
  const onPick = vi.fn();
  const result = render(
    <AgentTeamCanvas sessionId="ses_main" view={view} chosen={chosen} onPick={onPick} />,
  );
  const board = screen.getByRole('group', { name: 'Agent 画布' });
  const card = (name: string) => within(board).getByRole('button', { name: new RegExp(name, 'u') });
  const line = (agent: string) =>
    board.querySelector<SVGPathElement>(`.agent-canvas-line-path[data-agent="${agent}"]`);
  const flow = (agent: string) =>
    board.querySelector<SVGPathElement>(`.agent-canvas-flow[data-agent="${agent}"]`);
  return { ...result, onPick, board, card, line, flow };
}

test('already working sub-agents show a continuous flow when the canvas opens', () => {
  vi.useFakeTimers();
  const { flow, rerender, onPick } = shown(team(member('A1', '读代码')));
  expect(flow('A1')?.getAttribute('data-kind')).toBe('task');
  act(() => vi.advanceTimersByTime(FLOW_MS * 2));
  expect(flow('A1')?.getAttribute('data-kind')).toBe('task');
  rerender(
    <AgentTeamCanvas
      sessionId="ses_main"
      view={team(member('A1', '读代码', { working: false }))}
      chosen={undefined}
      onPick={onPick}
    />,
  );
  expect(flow('A1')).toBeNull();
});

test('each agent is a card with its facts, joined to the main agent by a line', () => {
  const { card, line } = shown(
    team(
      member('A1', '读代码', { tool: 'fs.read', tokens: 3456, approvals: ['apr_1'] }),
      member('A2', '改代码', {
        state: 'reported',
        working: false,
        last_report: 'ready',
        worktree: { branch: 'aura/agent/0a1b2c3d', state: 'active' },
      } as Partial<AgentTeamMemberView>),
      member('A3', '整理', { state: 'stopped', working: false }),
    ),
  );
  for (const part of ['主 Agent', 'GPT 6.1 Sol', '运行中', '12,000', '等子 Agent 报告'])
    expect(card('主 Agent').textContent).toContain(part);
  for (const part of ['读代码', 'A1', 'DeepSeek V4.1 Flash', '等审批', '3,456', '读取文件'])
    expect(card('读代码').textContent).toContain(part);
  // An approval it waits for marks its card; a worktree names its branch.
  expect(card('读代码').querySelector('.agent-canvas-badge')?.textContent).toBe('1');
  expect(card('改代码').textContent).toContain('aura/agent/0a1b2c3d');
  expect(card('改代码').textContent).toContain('已交报告');
  // A stopped one is drawn as ended, its line dashed.
  expect(card('整理').className).toContain('is-ended');
  expect(line('A3')?.getAttribute('data-ended')).toBe('true');
  for (const agent of ['A1', 'A2', 'A3'])
    expect(line(agent)?.getAttribute('d')).toMatch(/^M\d+ \d+( L\d+ \d+)+$/u);
});

test('picking a card picks its agent; the picked card and its line stand out', () => {
  const { card, line, onPick } = shown(team(member('A1', '读代码'), member('A2', '查测试')), 'A2');
  expect(card('查测试').getAttribute('aria-current')).toBe('true');
  expect(card('主 Agent').getAttribute('aria-current')).toBe('false');
  expect(line('A2')?.getAttribute('data-chosen')).toBe('true');
  expect(line('A1')?.getAttribute('data-chosen')).toBeNull();
  fireEvent.click(card('读代码'));
  expect(card('读代码').getAttribute('aria-pressed')).toBe('true');
  expect(onPick).not.toHaveBeenCalled();
  fireEvent.doubleClick(card('读代码'));
  expect(onPick).toHaveBeenLastCalledWith('A1');
  fireEvent.doubleClick(card('主 Agent'));
  expect(onPick).toHaveBeenLastCalledWith(undefined);
});

test('working lines point toward the child and the completed report pulses back once', () => {
  vi.useFakeTimers();
  const working = team(member('A1', '读代码'));
  const view = shown(working);
  expect(view.flow('A1')?.getAttribute('data-kind')).toBe('task');
  // A report goes up from the sub-agent: the light runs from its end of the line.
  const reported = team(member('A1', '读代码', { state: 'reported', last_report: 'ready' }));
  view.rerender(
    <AgentTeamCanvas
      sessionId="ses_main"
      view={reported}
      chosen={undefined}
      onPick={view.onPick}
    />,
  );
  const up = view.flow('A1');
  expect(up?.getAttribute('data-kind')).toBe('report');
  const line = view.line('A1')!.getAttribute('d')!;
  const end = line.split(' L').at(-1);
  expect(up?.getAttribute('d')?.startsWith(`M${end}`)).toBe(true);
  expect(view.board.querySelector('.agent-canvas-chip')?.textContent).toBe('报告');
  act(() => vi.advanceTimersByTime(FLOW_MS + 10));
  expect(view.flow('A1')).toBeNull();
  // A new sub-agent's task sheet goes down to it.
  const grown = team(
    member('A1', '读代码', { state: 'reported', last_report: 'ready' }),
    member('A2', '查测试'),
  );
  view.rerender(
    <AgentTeamCanvas sessionId="ses_main" view={grown} chosen={undefined} onPick={view.onPick} />,
  );
  const down = view.flow('A2');
  expect(down?.getAttribute('data-kind')).toBe('task');
  expect(down?.getAttribute('d')).toBe(view.line('A2')?.getAttribute('d'));
});

test('the zoom buttons zoom, and 全部显示 goes back to keeping every agent in sight', () => {
  const { board } = shown(team(member('A1', '读代码')));
  const level = () => board.querySelector('.agent-canvas-zoom span')?.textContent;
  expect(level()).toBe('100%');
  fireEvent.click(within(board).getByRole('button', { name: '放大' }));
  expect(level()).toBe('120%');
  expect(within(board).getByRole('button', { name: '全部显示' }).getAttribute('aria-pressed')).toBe(
    'false',
  );
  fireEvent.click(within(board).getByRole('button', { name: '缩小' }));
  expect(level()).toBe('100%');
  fireEvent.click(within(board).getByRole('button', { name: '全部显示' }));
  expect(within(board).getByRole('button', { name: '全部显示' }).getAttribute('aria-pressed')).toBe(
    'true',
  );
});

test('a card can be moved, and moving it does not pick it; its line follows', () => {
  const { board, card, line, onPick } = shown(team(member('A1', '读代码')));
  const before = { left: card('读代码').style.left, line: line('A1')?.getAttribute('d') };
  fireEvent.pointerDown(card('读代码'), { button: 0, clientX: 100, clientY: 100 });
  fireEvent.pointerMove(card('读代码'), { clientX: 160, clientY: 130 });
  fireEvent.pointerUp(card('读代码'));
  fireEvent.click(card('读代码'));
  expect(onPick).not.toHaveBeenCalled();
  expect(card('读代码').style.left).not.toBe(before.left);
  expect(line('A1')?.getAttribute('d')).not.toBe(before.line);
  // A press that does not move picks.
  fireEvent.pointerDown(card('读代码'), { button: 0, clientX: 10, clientY: 10 });
  fireEvent.pointerUp(card('读代码'));
  fireEvent.click(card('读代码'));
  expect(card('读代码').getAttribute('aria-pressed')).toBe('true');
  expect(onPick).not.toHaveBeenCalled();
  // Dragging the board moves the view, not the cards.
  const left = card('读代码').style.left;
  fireEvent.pointerDown(board, { button: 0, clientX: 10, clientY: 10 });
  fireEvent.pointerMove(board, { clientX: 50, clientY: 30 });
  fireEvent.pointerUp(board);
  expect(card('读代码').style.left).toBe(left);
  expect((board.querySelector('.agent-canvas-world') as HTMLElement).style.transform).toContain(
    'translate(40px, 20px)',
  );
});

test('the width of the panel moves only the view: every card keeps its place, moved or not', () => {
  const resize = sizedBoard(610);
  const { card } = shown(team(member('A1', '读代码'), member('A2', '交互'), member('A3', '视觉')));
  const offset = (name: string) => [
    parseFloat(card(name).style.left) - parseFloat(card('主 Agent').style.left),
    parseFloat(card(name).style.top) - parseFloat(card('主 Agent').style.top),
  ];
  fireEvent.pointerDown(card('读代码'), { button: 0, clientX: 100, clientY: 100 });
  fireEvent.pointerMove(card('读代码'), { clientX: 140, clientY: 120 });
  fireEvent.pointerUp(card('读代码'));
  const names = ['读代码', '交互', '视觉'];
  const before = names.map(offset);
  for (const width of [795, 1606, 412]) {
    resize(width);
    expect(names.map(offset), `${width} wide`).toEqual(before);
  }
});

test('a press that shakes a few pixels picks the card and leaves it where it was', () => {
  const { card, onPick } = shown(team(member('A1', '读代码')));
  const left = card('读代码').style.left;
  fireEvent.pointerDown(card('读代码'), { button: 0, clientX: 10, clientY: 10 });
  fireEvent.pointerMove(card('读代码'), { clientX: 13, clientY: 11 });
  fireEvent.pointerUp(card('读代码'));
  fireEvent.click(card('读代码'));
  expect(card('读代码').getAttribute('aria-pressed')).toBe('true');
  expect(onPick).not.toHaveBeenCalled();
  expect(card('读代码').style.left).toBe(left);
});

test('重新排列 puts every moved card back and shows them all again', () => {
  const { board, card } = shown(team(member('A1', '读代码'), member('A2', '查测试')));
  const left = card('读代码').style.left;
  fireEvent.pointerDown(card('读代码'), { button: 0, clientX: 100, clientY: 100 });
  fireEvent.pointerMove(card('读代码'), { clientX: 180, clientY: 160 });
  fireEvent.pointerUp(card('读代码'));
  expect(card('读代码').style.left).not.toBe(left);
  const all = within(board).getByRole('button', { name: '全部显示' });
  expect(all.getAttribute('aria-pressed')).toBe('false');
  fireEvent.click(within(board).getByRole('button', { name: '重新排列' }));
  expect(card('读代码').style.left).toBe(left);
  expect(all.getAttribute('aria-pressed')).toBe('true');
});

test('a sub-agent card has a menu: its worktree in the file manager, its mode, its grants', async () => {
  const open = vi
    .fn()
    .mockResolvedValue({ ok: true, value: { completed: true, message: '已打开' } });
  const state = vi.fn().mockResolvedValue({
    ok: true,
    value: {
      session_id: 'ses_A1',
      mode: 'workspace_read_write',
      revision: 3,
      active_grant_count: 0,
    },
  });
  const setMode = vi.fn().mockResolvedValue({ ok: true, value: {} });
  vi.stubGlobal('aura', {
    v1: {
      openAgentTeamWorktree: open,
      getSessionPermissionState: state,
      setSessionPermissionMode: setMode,
      listAgentTeam: vi.fn(() => new Promise(() => {})),
    },
  });
  const { card } = shown(
    team(
      member('A1', '改代码', {
        mode: 'workspace_read_write',
        workspace_id: 'wsp_A1',
        worktree: { branch: 'aura/agent/0a1b2c3d', state: 'active' },
      } as Partial<AgentTeamMemberView>),
      member('A2', '读代码', { mode: 'read_only', workspace_id: 'wsp_main' }),
    ),
  );
  fireEvent.contextMenu(card('改代码'));
  const menu = await screen.findByRole('menu', { name: '改代码' });
  const items = within(menu)
    .getAllByRole('menuitem')
    .map((item) => item.textContent);
  expect(items).toEqual(['在文件管理器中查看工作区', '只读', '工作区读写', '完全访问', '授权管理']);
  fireEvent.click(within(menu).getByRole('menuitem', { name: '在文件管理器中查看工作区' }));
  await act(async () => {});
  expect(open).toHaveBeenCalledWith({ session_id: 'ses_main', agent: 'A1' });

  // One working in the conversation's workspace has no worktree to show.
  fireEvent.contextMenu(card('读代码'));
  const second = await screen.findByRole('menu', { name: '读代码' });
  expect(within(second).queryByRole('menuitem', { name: '在文件管理器中查看工作区' })).toBeNull();
  fireEvent.click(within(second).getByRole('menuitem', { name: '工作区读写' }));
  await act(async () => {});
  expect(setMode).toHaveBeenCalledWith({
    session_id: 'ses_A2',
    expected_revision: 3,
    mode: 'workspace_read_write',
  });

  // 完全访问 waits for its confirmation.
  fireEvent.contextMenu(card('读代码'));
  const third = await screen.findByRole('menu', { name: '读代码' });
  fireEvent.click(within(third).getByRole('menuitem', { name: '完全访问' }));
  await act(async () => {});
  expect(setMode).toHaveBeenCalledTimes(1);
  fireEvent.click(await screen.findByRole('button', { name: '启用完全访问' }));
  await act(async () => {});
  expect(setMode).toHaveBeenLastCalledWith({
    session_id: 'ses_A2',
    expected_revision: 3,
    mode: 'full_access',
  });
});
