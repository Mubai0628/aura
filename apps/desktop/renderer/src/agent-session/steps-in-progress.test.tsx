// eslint-disable-next-line no-restricted-imports -- a test reads its stylesheet: Vitest hands a CSS import over empty, and the rule is for the renderer's own code.
import { readFileSync } from 'node:fs';
import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ReasoningTrace } from './reasoning-trace';
import { ToolActivity } from './ToolActivity';

// A step still under way — 思考过程, 运行命令, 写入文件 … — carries a dim,
// slanted sweep of light from left to right; a long thought can be closed from where it ends.

beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentMessageText: vi.fn().mockResolvedValue({ ok: false, error: { message: 'stop' } }),
      },
    },
  });
});
const original = Element.prototype.scrollIntoView;
afterEach(() => {
  cleanup();
  Element.prototype.scrollIntoView = original;
});

const call = (status: string, outcome: string | null) =>
  ({
    display: {
      details: [],
      internal: false,
      summary: '',
      target: 'notes/plan.md',
      title: '写入文件',
      truncated: false,
    },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: 'fs.write',
      ordinal: 1,
      outcome: outcome ? { type: outcome } : null,
      side_effect_state: null,
      started_at: null,
      status,
      tool_batch_id: 'tbt_1',
      tool_call_id: 'tcl_01HOSTTOOLCALL0000000001',
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
  }) as unknown as AgentToolActivityEntry;

const thought = (generating: boolean) =>
  render(
    <ReasoningTrace
      sessionId="session"
      modelStepId="step"
      preview={'先看结构。\n再写页面。'}
      totalBytes={30}
      partial={generating}
      generating={generating}
      bodyId={null}
    />,
  );

test('a call under way sweeps; a finished one and one waiting for you do not', () => {
  const running = render(<ToolActivity entry={call('running', null)} />);
  expect(running.container.querySelector('.agent-tool-row')?.getAttribute('data-running')).toBe(
    'true',
  );
  running.unmount();
  for (const [status, outcome] of [
    ['finalized', 'succeeded'],
    ['waiting_approval', null],
  ] as const) {
    const shown = render(<ToolActivity entry={call(status, outcome)} />);
    expect(shown.container.querySelector('[data-running]')).toBeNull();
    shown.unmount();
  }
});

// The light passes
// through the words of a step under way; no layer is laid over the row.
test('the sweep is in the words of a step, not over its row', () => {
  const running = render(<ToolActivity entry={call('running', null)} />);
  expect(running.container.querySelector('.agent-tool-title')?.textContent).toBeTruthy();
  running.unmount();
  const writing = thought(true);
  expect(writing.container.querySelector('summary .agent-thinking-label')?.textContent).toBe(
    '思考过程',
  );
  writing.unmount();
  // Vitest hands a CSS import over empty, so the stylesheet is read as a file.
  const processCss = readFileSync(`${__dirname}/turn-process.css`, 'utf8');
  const rules = processCss.replace(/\/\*[\s\S]*?\*\//gu, '');
  expect(rules).not.toMatch(/\[data-running='true'\]::after/u);
  const sweep = rules.match(/([^{}]*)\{[^{}]*background-clip: text[^{}]*\}/u);
  expect(sweep?.[1]).toMatch(/\.agent-tool-title/u);
  expect(sweep?.[1]).toMatch(/\.agent-thinking-label/u);
  expect(sweep?.[1]).toMatch(/\.agent-process-label/u);
  expect(sweep?.[1]).not.toMatch(/agent-file-label/u);
});

test('a thought being written sweeps; a finished one does not', () => {
  const writing = thought(true);
  expect(writing.container.querySelector('summary')?.getAttribute('data-running')).toBe('true');
  writing.unmount();
  const done = thought(false);
  expect(done.container.querySelector('[data-running]')).toBeNull();
});

test('a long thought closes from its end, and its title comes into view', async () => {
  const user = userEvent.setup();
  // jsdom lays nothing out and has no scrollIntoView: the call itself is what is checked.
  const scrolled = vi.fn();
  Element.prototype.scrollIntoView = scrolled;
  const view = thought(false);
  const details = view.container.querySelector('details')!;
  expect(screen.queryByRole('button', { name: '收起思考过程' })).toBeNull();
  details.open = true;
  fireEvent(details, new Event('toggle'));
  await user.click(within(details).getByRole('button', { name: '收起思考过程' }));
  expect(details.open).toBe(false);
  fireEvent(details, new Event('toggle'));
  expect(scrolled.mock.contexts.at(-1)).toBe(details.querySelector('summary'));
  expect(screen.queryByRole('button', { name: '收起思考过程' })).toBeNull();
});
