import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ExpandIcon } from '../tool-icon';
import { ReasoningTrace } from './reasoning-trace';
import { ToolActivity } from './ToolActivity';
import { ToolCallRun } from './tool-run-row';
import { useDisclosureOpen } from './disclosure-open';

afterEach(cleanup);

/** The rendered glyph name of the tool row's disclosure slot. */
const iconName = (container: Element): string | null | undefined =>
  container.querySelector('.agent-tool-chevron [data-icon]')?.getAttribute('data-icon');

/** The same, for the disclosure slots that use the shared `.agent-chevron`. */
const chevronName = (container: Element): string | null | undefined =>
  container.querySelector('.agent-chevron [data-icon]')?.getAttribute('data-icon');

/** `toggle` is the DOM event a `<details>` fires once its state really changed;
 *  jsdom does not fire it by itself, so the test fires what a browser would. */
const toggleTo = (details: HTMLDetailsElement, open: boolean): void => {
  details.open = open;
  fireEvent(details, new Event('toggle'));
};

const entry = (details: string[]): AgentToolActivityEntry =>
  ({
    display: {
      details,
      internal: false,
      summary: '文件共 2 行',
      target: 'note.txt',
      title: '读取文件',
      truncated: false,
    },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: 'fs.read_text',
      ordinal: 1,
      outcome: { type: 'succeeded' },
      side_effect_state: null,
      started_at: null,
      status: 'finalized',
      tool_batch_id: 'tbt_1',
      tool_call_id: 'tcl_01HOSTTOOLCALL0000000001',
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
  }) as unknown as AgentToolActivityEntry;

test('the expand icon picks its glyph from the open state', () => {
  const closed = render(<ExpandIcon open={false} />);
  expect(closed.container.querySelector('[data-icon]')?.getAttribute('data-icon')).toBe(
    'chevron-right',
  );
  closed.unmount();
  const open = render(<ExpandIcon open={true} />);
  expect(open.container.querySelector('[data-icon]')?.getAttribute('data-icon')).toBe(
    'chevron-down',
  );
});

test('a tool row points right while collapsed and down while expanded', () => {
  const view = render(<ToolActivity entry={entry(['第一行诊断'])} />);
  const details = view.container.querySelector('details')!;
  expect(iconName(view.container)).toBe('chevron-right');
  expect(details.hasAttribute('open')).toBe(false);

  toggleTo(details, true);

  expect(iconName(view.container)).toBe('chevron-down');
  expect(screen.getByText('第一行诊断')).toBeDefined();
});

test('a tool row with nothing to expand shows no disclosure', () => {
  const view = render(<ToolActivity entry={entry([])} />);
  expect(view.container.querySelector('summary')).toBeNull();
  // The arrow sits at the end of the line and is left out when there is nothing to open.
  expect(view.container.querySelector('.agent-tool-chevron')).toBeNull();
  // The category icon still occupies a fixed slot, so titles stay aligned.
  expect(view.container.querySelector('.agent-tool-kind')?.getAttribute('data-icon')).toBe(
    'file-text',
  );
});

test('the disclosure hook reports the real toggle state after React state changes', () => {
  function Probe() {
    const { onToggle, open } = useDisclosureOpen();
    return (
      <details onToggle={onToggle}>
        <summary>探测</summary>
        <span data-testid="state">{open ? 'open' : 'closed'}</span>
      </details>
    );
  }
  render(<Probe />);
  const details = document.querySelector('details')!;
  expect(screen.getByTestId('state').textContent).toBe('closed');
  toggleTo(details, true);
  expect(screen.getByTestId('state').textContent).toBe('open');
  toggleTo(details, false);
  expect(screen.getByTestId('state').textContent).toBe('closed');
});

test('the reasoning trace points right while collapsed and down while expanded', () => {
  const read = vi.fn().mockResolvedValue({ ok: false, error: { message: 'stop' } });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText: read } },
  });
  const view = render(
    <ReasoningTrace
      sessionId="session"
      modelStepId="step"
      preview="因为"
      totalBytes={6}
      partial={false}
      bodyId={null}
    />,
  );
  const details = view.container.querySelector('details')!;
  expect(chevronName(view.container)).toBe('chevron-right');
  toggleTo(details, true);
  expect(chevronName(view.container)).toBe('chevron-down');
});

test('a grouped tool run points right while collapsed and down while expanded', () => {
  const view = render(
    <ToolCallRun
      group={{
        callIds: ['a', 'b'],
        displayName: 'fs.read_text',
        finalStateKey: 'succeeded',
        openKey: '',
        identity: 'fs.read_text',
        key: 'a',
        state: '成功',
        title: '读取文件',
        tone: 'success',
      }}
      calls={new Map()}
    />,
  );
  const details = view.container.querySelector('details')!;
  expect(iconName(view.container)).toBe('chevron-right');
  toggleTo(details, true);
  expect(iconName(view.container)).toBe('chevron-down');
});
