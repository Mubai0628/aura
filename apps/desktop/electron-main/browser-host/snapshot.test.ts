// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { RefTable, renderSnapshot, type AxNode } from './snapshot.js';

const node = (
  nodeId: string,
  role: string,
  name: string,
  childIds: string[] = [],
  extra: Partial<AxNode> = {},
): AxNode => ({
  nodeId,
  role: { value: role },
  name: { value: name },
  childIds,
  backendDOMNodeId: Number(nodeId) + 100,
  ...extra,
});

// Shaped like Accessibility.getFullAXTree on a small game page.
const page: AxNode[] = [
  node('1', 'RootWebArea', 'Minesweeper', ['2']),
  { ...node('2', 'generic', '', ['3', '5', '7', '9', '11', '12']), parentId: '1' },
  {
    ...node('3', 'heading', 'Minesweeper', ['4']),
    parentId: '2',
    properties: [{ name: 'level', value: { value: 1 } }],
  },
  { ...node('4', 'StaticText', 'Minesweeper'), parentId: '3' },
  { ...node('5', 'button', 'Start', ['6']), parentId: '2' },
  { ...node('6', 'StaticText', 'Start', ['13']), parentId: '5' },
  { ...node('13', 'InlineTextBox', 'Start'), parentId: '6' },
  { ...node('7', 'textbox', 'Name'), parentId: '2', value: { value: 'Ada' } },
  {
    ...node('9', 'textbox', ''),
    parentId: '2',
    value: { value: 'hunter2' },
    properties: [{ name: 'autocomplete', value: { value: 'current-password' } }],
  },
  { ...node('11', 'Canvas', ''), parentId: '2' },
  {
    ...node('12', 'checkbox', 'Sound'),
    parentId: '2',
    properties: [{ name: 'checked', value: { value: 'true' } }],
  },
  { ...node('20', 'button', 'Hidden'), parentId: '2', ignored: true },
];

describe('the page snapshot', () => {
  it('shows roles, names, values and states, with refs on what can be acted on', () => {
    const refs = new RefTable();
    const { text, truncated } = renderSnapshot(
      page,
      refs,
      10_000,
      new Map([[111, { x: 40, y: 120, width: 400, height: 300 }]]),
    );
    expect(truncated).toBe(false);
    expect(text.split('\n')).toEqual([
      '- heading "Minesweeper" [level=1]',
      '- button "Start" [ref=e1]',
      '- textbox "Name" value="Ada" [ref=e2]',
      '- textbox value="••••" [ref=e3]',
      '- Canvas [ref=e4] (at 40,120 size 400x300)',
      '- checkbox "Sound" [checked] [ref=e5]',
    ]);
  });

  it('keeps each element its ref from one snapshot to the next', () => {
    const refs = new RefTable();
    renderSnapshot(page, refs, 10_000);
    const reordered = renderSnapshot(
      [page[0]!, { ...page[1]!, childIds: ['12', '5'] }, ...page.slice(2)],
      refs,
      10_000,
    );
    expect(reordered.text).toContain('checkbox "Sound" [checked] [ref=e5]');
    expect(reordered.text).toContain('button "Start" [ref=e1]');
    expect(refs.node('e1')).toBe(105);
    refs.clear();
    expect(refs.node('e1')).toBeUndefined();
    // A ref from before is never reused: an old e1 cannot name a new element.
    expect(refs.refFor(105)).toBe('e6');
  });

  it('stops at a whole line when it is too long and says so', () => {
    const { text, truncated } = renderSnapshot(page, new RefTable(), 55);
    expect(truncated).toBe(true);
    expect(text).toBe('- heading "Minesweeper" [level=1]');
  });
});
