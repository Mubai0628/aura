import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { graphView } from '../graph-fixtures.test-support';
import { AgentGraphCanvas } from './AgentGraphCanvas';
import { graphPositions, positionsPayload } from './graph-layout';

beforeEach(() => {
  localStorage.clear();
  vi.stubGlobal(
    'PointerEvent',
    class extends MouseEvent {
      readonly pointerId = 1;
    },
  );
  Element.prototype.setPointerCapture = vi.fn();
});

test('selected parent nodes move together with both temporary teams in one revision', async () => {
  boardSize();
  const { view, change, card } = shown();
  const other = screen.getByRole('button', { name: /^开发Fixture/u });
  const old = graphPositions(view);
  await act(async () => {
    for (const node of [card, other]) fireEvent.keyDown(node, { key: ' ', ctrlKey: true });
    fireEvent.keyDown(other, { key: 'ArrowRight' });
  });
  expect(change).toHaveBeenCalledTimes(1);
  expect(change).toHaveBeenCalledWith(
    {
      action: 'place',
      positions: expect.arrayContaining(
        positionsPayload(
          Object.fromEntries(Object.entries(old).map(([id, at]) => [id, { x: at.x + 8, y: at.y }])),
        ),
      ),
    },
    { graph_id: 'acg_one', revision: 7 },
  );
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  for (const name of ['clientWidth', 'clientHeight'])
    if (Object.hasOwn(HTMLElement.prototype, name))
      delete (HTMLElement.prototype as unknown as Record<string, unknown>)[name];
});
function boardSize(initial = 1600) {
  let width = initial,
    notify: (() => void) | undefined;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        notify = callback;
      }
      observe() {}
      disconnect() {}
    },
  );
  for (const [name, pixels] of [
    ['clientWidth', () => width],
    ['clientHeight', () => 700],
  ] as const)
    Object.defineProperty(HTMLElement.prototype, name, {
      configurable: true,
      get(this: HTMLElement) {
        return this.classList.contains('agent-canvas') ? pixels() : 0;
      },
    });
  return (next: number) => {
    width = next;
    act(() => notify?.());
  };
}
function shown() {
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  const change = vi.fn().mockResolvedValue(true),
    pick = vi.fn();
  render(
    <AgentGraphCanvas
      view={view}
      sessionId="ses_a"
      chosen={undefined}
      disabled={false}
      change={change}
      onPick={pick}
      onRemove={vi.fn()}
      onAdd={vi.fn()}
      onReset={vi.fn()}
      onRefresh={vi.fn()}
      onEditProfile={vi.fn()}
    />,
  );
  return {
    view,
    change,
    pick,
    board: screen.getByRole('group', { name: 'Agent 画布' }),
    card: screen.getByRole('button', { name: /^规划Fixture/u }),
  };
}

test('batched pointer moves save the latest world point once at the captured revision', async () => {
  boardSize();
  const { view, change, board, card } = shown();
  fireEvent.click(screen.getByRole('button', { name: '放大' }));
  expect(screen.getByLabelText('画布缩放比例').textContent).toBe('120%');
  await act(async () => {
    card.dispatchEvent(
      new PointerEvent('pointerdown', { bubbles: true, button: 0, clientX: 0, clientY: 0 }),
    );
    board.dispatchEvent(
      new PointerEvent('pointermove', { bubbles: true, clientX: 72, clientY: 48 }),
    );
    board.dispatchEvent(new PointerEvent('pointerup', { bubbles: true, clientX: 72, clientY: 48 }));
  });
  expect(change).toHaveBeenCalledTimes(1);
  const old = graphPositions(view);
  expect(change).toHaveBeenCalledWith(
    {
      action: 'place',
      positions: positionsPayload({
        ses_a: { x: old.ses_a!.x + 60, y: old.ses_a!.y + 40 },
        ses_a_child: { x: old.ses_a_child!.x + 60, y: old.ses_a_child!.y + 40 },
      }),
    },
    { graph_id: 'acg_one', revision: 7 },
  );
});

test('resize zoom pan and a cancelled drag never rewrite saved node coordinates', () => {
  const resize = boardSize(320);
  const { change, board, card } = shown();
  const before = { left: card.style.left, top: card.style.top };
  resize(1600);
  fireEvent.click(screen.getByRole('button', { name: '放大' }));
  fireEvent.keyDown(board, { key: 'ArrowLeft' });
  expect({ left: card.style.left, top: card.style.top }).toEqual(before);
  fireEvent.pointerDown(card, { button: 0, clientX: 10, clientY: 10 });
  fireEvent.pointerMove(board, { clientX: 150, clientY: 120 });
  fireEvent.pointerCancel(board);
  expect({ left: card.style.left, top: card.style.top }).toEqual(before);
  expect(change).not.toHaveBeenCalled();
});

test('Ctrl pressed during a drag aligns the group immediately and the aligned drop saves once', async () => {
  boardSize();
  const { view, change, board, card } = shown();
  const old = graphPositions(view);
  fireEvent.click(screen.getByRole('button', { name: '放大' }));
  fireEvent.pointerDown(card, { button: 0, clientX: 0, clientY: 0 });
  fireEvent.pointerMove(board, { clientX: 72, clientY: 7 });
  expect(card.style.top).toBe(`${old.ses_a!.y + 6}px`);

  fireEvent.keyDown(card, { key: 'Control', ctrlKey: true });
  expect(card.style.top).toBe(`${old.ses_b!.y}px`);
  fireEvent.keyUp(card, { key: 'Control' });
  expect(card.style.top).toBe(`${old.ses_a!.y + 6}px`);

  fireEvent.keyDown(card, { key: 'Control', ctrlKey: true });
  await act(async () => {
    fireEvent.pointerUp(board, { clientX: 72, clientY: 7, ctrlKey: true });
  });
  expect(change).toHaveBeenCalledTimes(1);
  expect(change).toHaveBeenCalledWith(
    {
      action: 'place',
      positions: positionsPayload({
        ses_a: { x: old.ses_a!.x + 60, y: old.ses_a!.y },
        ses_a_child: { x: old.ses_a_child!.x + 60, y: old.ses_a_child!.y },
      }),
    },
    { graph_id: 'acg_one', revision: 7 },
  );
});

test('identically named A1 cards select locally and double click opens the real parent and child', () => {
  boardSize();
  const { pick, change } = shown();
  const child = screen.getByRole('button', { name: /^开发的 A1/u });
  fireEvent.click(child);
  expect(child.getAttribute('aria-pressed')).toBe('true');
  expect(pick).not.toHaveBeenCalled();
  fireEvent.doubleClick(child);
  expect(pick).toHaveBeenCalledWith('ses_b', 'A1');
  expect(change).not.toHaveBeenCalled();
});

test('temporary children can be selected together and moved without moving their parents', async () => {
  boardSize();
  const { view, change } = shown();
  const children = screen.getAllByRole('button', { name: /的 A1/u });
  await act(async () => {
    for (const card of children) fireEvent.keyDown(card, { key: ' ', ctrlKey: true });
    fireEvent.keyDown(children[0]!, { key: 'ArrowRight' });
  });
  const old = graphPositions(view);
  expect(change).toHaveBeenCalledWith(
    {
      action: 'place',
      positions: positionsPayload({
        ses_a_child: { ...old.ses_a_child!, x: old.ses_a_child!.x + 8 },
        ses_b_child: { ...old.ses_b_child!, x: old.ses_b_child!.x + 8 },
      }),
    },
    { graph_id: 'acg_one', revision: 7 },
  );
});

test('stopped agents fold on first display, stay expanded when reopened, and can be folded individually', () => {
  boardSize();
  const view = graphView([
    ['ses_a', '规划'],
    ['ses_b', '开发'],
  ]);
  view.teams[0]!.team.members[0]!.state = 'stopped';
  const panel = () => (
    <AgentGraphCanvas
      view={view}
      sessionId="ses_a"
      chosen={undefined}
      disabled={false}
      change={vi.fn().mockResolvedValue(true)}
      onPick={vi.fn()}
      onRemove={vi.fn()}
      onAdd={vi.fn()}
      onReset={vi.fn()}
      onRefresh={vi.fn()}
      onEditProfile={vi.fn()}
    />
  );
  const first = render(panel());
  expect(screen.queryByRole('button', { name: /^规划的 A1/ })).toBeNull();
  expect(screen.getByRole('button', { name: /^开发的 A1/ })).toBeTruthy();
  fireEvent.contextMenu(screen.getByRole('button', { name: /^规划Fixture/ }));
  fireEvent.pointerEnter(screen.getByRole('menuitem', { name: '展开子 Agent' }));
  const stoppedType = screen.getByRole('menuitem', { name: '已停止（1）' });
  stoppedType.focus();
  fireEvent.keyDown(stoppedType, { key: 'ArrowRight' });
  expect(document.activeElement).toBe(screen.getByRole('menuitem', { name: '展开全部' }));
  fireEvent.click(screen.getByRole('menuitem', { name: '展开全部' }));
  const position = screen.getByRole('button', { name: /^规划的 A1/ }).style.left;
  first.unmount();
  render(panel());
  const stopped = screen.getByRole('button', { name: /^规划的 A1/ });
  expect(stopped.style.left).toBe(position);
  const reported = screen.getByRole('button', { name: /^开发的 A1/ });
  fireEvent.contextMenu(reported);
  fireEvent.click(screen.getByRole('menuitem', { name: '收起子 Agent' }));
  expect(screen.queryByRole('button', { name: /^开发的 A1/ })).toBeNull();
  fireEvent.contextMenu(screen.getByRole('button', { name: /^开发Fixture/ }));
  fireEvent.pointerEnter(screen.getByRole('menuitem', { name: '展开子 Agent' }));
  expect(screen.getByRole('menuitem', { name: '展开 A1 检查' })).toBeTruthy();
});

test('link mode holds positions and Shift-drag connects without leaving a mode open', async () => {
  boardSize();
  const { change, pick, board, card } = shown();
  const second = screen.getByRole('button', { name: /^开发Fixture/u });
  expect(screen.queryByRole('button', { name: '规划发起连线' })).toBeNull();
  fireEvent.contextMenu(card);
  fireEvent.click(screen.getByRole('menuitem', { name: '发起连线' }));
  fireEvent.keyDown(card, { key: 'ArrowRight' });
  expect(change).not.toHaveBeenCalled();
  fireEvent.click(second);
  expect(change).toHaveBeenLastCalledWith(
    { action: 'connect', source_session_id: 'ses_a', target_session_id: 'ses_b' },
    { graph_id: 'acg_one', revision: 7 },
  );
  expect(screen.getByRole('button', { name: '取消连线' })).toBeTruthy();
  fireEvent.keyDown(board, { key: 'Escape' });
  fireEvent.pointerDown(second, { button: 0, shiftKey: true, clientX: 20, clientY: 20 });
  fireEvent.pointerMove(board, { clientX: 100, clientY: 85 });
  expect(board.querySelector('.agent-graph-link-preview line')).toBeTruthy();
  fireEvent.pointerUp(card, { clientX: 100, clientY: 85 });
  expect(change).toHaveBeenLastCalledWith(
    { action: 'connect', source_session_id: 'ses_b', target_session_id: 'ses_a' },
    { graph_id: 'acg_one', revision: 7 },
  );
  expect(screen.queryByRole('button', { name: '取消连线' })).toBeNull();
  expect(pick).not.toHaveBeenCalled();
  expect(change).toHaveBeenCalledTimes(2);
});
