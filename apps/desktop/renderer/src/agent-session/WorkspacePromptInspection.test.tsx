import type {
  AgentPromptInspection,
  AgentSessionProjection,
  ContextUsageSummaryView,
} from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { WorkspaceView } from '../WorkspaceView';
import { resetCoreHealthStoreForTests } from '../core-health-store';
import { resetToolTabsForTests } from '../right-workspace/tool-tabs';
import { resetPromptInspectionsForTests } from './usePromptInspection';

const usage = (tokens: number, estimated = true): ContextUsageSummaryView => ({
  estimated,
  runtime_effective_window_tokens: 1000,
  used_input_tokens: tokens,
  segments: [{ kind: 'conversation', label: '对话', estimated_tokens: tokens }],
});
function session(step = 'step_1', meter = usage(100), sessionId = 'session', runtime = 'runtime') {
  return {
    session_id: sessionId,
    runtime_instance_id: runtime,
    revision: '1',
    inbox: { messages: [] },
    turns: [
      {
        assistant_messages: [
          {
            model_step_id: step,
            context_snapshot_id: `snapshot_${step}`,
            context_usage: meter,
            context_compaction_running: false,
          },
        ],
      },
    ],
  } as unknown as AgentSessionProjection;
}
const inspection = (step: string, sessionId = 'session', tokens = 100) =>
  ({
    execution: { model_step_id: step },
    projection: { session_id: sessionId, context_usage: usage(tokens) },
  }) as unknown as AgentPromptInspection;
let projection: AgentSessionProjection;
const read = vi.fn();

vi.mock('./useAgentSession', () => ({
  useAgentSession: () => ({
    projection,
    loading: false,
    refresh: vi.fn(),
    loadOlder: vi.fn(),
    loadTurnOlder: vi.fn(),
  }),
}));
vi.mock('../model-projection-store', () => ({
  useScopedModelProjection: () => undefined,
  useModelProjectionError: () => undefined,
}));
vi.mock('../ui-settings-store', () => ({
  useUiSettings: () => ({ settings: { show_debug_information: false } }),
}));
vi.mock('../WorkspaceSplitPane', () => ({
  WorkspaceSplitPane: ({
    children,
    right,
  }: {
    children: React.ReactNode;
    right: React.ReactNode;
  }) => (
    <>
      {children}
      {right}
    </>
  ),
}));
vi.mock('./Conversation', () => ({
  Conversation: () => <div />,
}));
vi.mock('../ResizableComposer', () => ({
  ResizableComposer: ({ onOpenPromptInspector }: { onOpenPromptInspector: () => void }) => (
    <button onClick={onOpenPromptInspector}>打开上下文详情</button>
  ),
}));
vi.mock('../RightWorkspace', () => ({
  RightWorkspace: ({
    inspection: value,
    inspectionError,
  }: {
    inspection?: AgentPromptInspection;
    inspectionError?: string;
  }) => (
    <aside>
      <output aria-label="详情步骤">{value?.execution.model_step_id ?? 'empty'}</output>
      <output aria-label="详情用量">
        {value?.projection.context_usage?.used_input_tokens ?? 'empty'}
      </output>
      {inspectionError ? <p role="alert">{inspectionError}</p> : null}
    </aside>
  ),
}));

function view(id = 'session', sidebarOpen = true) {
  return (
    <WorkspaceView
      workspaceId="workspace"
      workspaceRoot="C:\\work"
      demoMode
      onOpenModelSettings={vi.fn()}
      onOpenSecuritySettings={vi.fn()}
      security={{} as never}
      onSetPermissionMode={vi.fn()}
      permissionMode="read_only"
      rightPanelOpen={sidebarOpen}
      rightPanelWidth={400}
      runtimeNotice={undefined}
      selectedSession={{ id, title: '对话' } as never}
      setRightPanelOpen={vi.fn()}
      setRightPanelWidth={vi.fn()}
      writeRightPanelWidth={vi.fn()}
      onNotify={vi.fn()}
    />
  );
}
function deferred() {
  let resolve!: (value: { ok: true; value: AgentPromptInspection }) => void;
  return {
    promise: new Promise<{ ok: true; value: AgentPromptInspection }>((done) => {
      resolve = done;
    }),
    resolve: (value: AgentPromptInspection) => resolve({ ok: true, value }),
  };
}
beforeEach(() => {
  resetToolTabsForTests();
  resetPromptInspectionsForTests();
  resetCoreHealthStoreForTests();
  projection = session();
  read
    .mockReset()
    .mockImplementation(
      async ({ model_step_id, session_id }: { model_step_id: string; session_id: string }) => ({
        ok: true,
        value: inspection(model_step_id, session_id),
      }),
    );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentInspector: read,
        // The view's waiting notice follows Core's health; it stays unknown here.
        onCoreHealth: () => () => undefined,
        getCoreHealth: () => new Promise(() => undefined),
      },
    },
  });
});
afterEach(cleanup);

test('context details follow a new request including compaction and the next turn, retaining old content while loading', async () => {
  const pending = deferred();
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
  read.mockReturnValueOnce(pending.promise);
  projection = session('step_2');
  projection.turns[0]!.assistant_messages[0]!.context_compaction_running = true;
  rendered.rerender(view());
  await waitFor(() =>
    expect(read).toHaveBeenLastCalledWith({ session_id: 'session', model_step_id: 'step_2' }),
  );
  expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1');
  await act(async () => {
    pending.resolve(inspection('step_2'));
  });
  expect(screen.getByLabelText('详情步骤').textContent).toBe('step_2');
  projection = { ...projection, turns: [...projection.turns, ...session('step_3').turns] };
  rendered.rerender(view());
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_3'));
});

test('details that are not on screen read nothing until they are shown again', async () => {
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  // The sidebar closed: newer requests are not read while the details cannot be seen.
  projection = session('step_2');
  rendered.rerender(view('session', false));
  projection = session('step_3');
  rendered.rerender(view('session', false));
  await act(async () => undefined);
  expect(read).toHaveBeenCalledTimes(1);
  // Shown again: the latest request is read once.
  rendered.rerender(view('session', true));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_3'));
  expect(read).toHaveBeenCalledTimes(2);
});

test('a changed meter on the same step is reloaded, while equal meter values do not reload', async () => {
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  read.mockResolvedValueOnce({ ok: true, value: inspection('step_1', 'session', 123) });
  projection = session('step_1', usage(123, false));
  rendered.rerender(view());
  await waitFor(() => expect(screen.getByLabelText('详情用量').textContent).toBe('123'));
  expect(read).toHaveBeenCalledTimes(2);
  projection = session('step_1', usage(123, false));
  projection.revision = '2';
  projection.turns[0]!.assistant_messages[0]!.text = 'streamed answer changed';
  rendered.rerender(view());
  expect(read).toHaveBeenCalledTimes(2);
});

test('the step is read again once its request finishes, when its cache use is known', async () => {
  projection = session();
  projection.turns[0]!.assistant_messages[0]!.partial = true;
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  // Still answering: streamed text alone reads nothing.
  projection = session();
  projection.turns[0]!.assistant_messages[0]!.partial = true;
  projection.turns[0]!.assistant_messages[0]!.text = 'streaming';
  rendered.rerender(view());
  await act(async () => undefined);
  expect(read).toHaveBeenCalledTimes(1);
  projection = session();
  projection.turns[0]!.assistant_messages[0]!.partial = false;
  rendered.rerender(view());
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
});

test('estimation, segment and window changes refresh even when the total token count is unchanged', async () => {
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  projection = session('step_1', usage(100, false));
  rendered.rerender(view());
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  const changed = usage(100, false);
  changed.segments[0]!.estimated_tokens = 90;
  projection = session('step_1', changed);
  rendered.rerender(view());
  await waitFor(() => expect(read).toHaveBeenCalledTimes(3));
  projection = session('step_1', { ...changed, runtime_effective_window_tokens: 2000 });
  rendered.rerender(view());
  await waitFor(() => expect(read).toHaveBeenCalledTimes(4));
});

test('a changed snapshot or compaction state refreshes the same step, even when token totals stay equal', async () => {
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(read).toHaveBeenCalledTimes(1));
  projection = session();
  projection.turns[0]!.assistant_messages[0]!.context_snapshot_id = 'snapshot_replaced';
  rendered.rerender(view());
  await waitFor(() => expect(read).toHaveBeenCalledTimes(2));
  projection = session();
  projection.turns[0]!.assistant_messages[0]!.context_snapshot_id = 'snapshot_replaced';
  projection.turns[0]!.assistant_messages[0]!.context_compaction_running = true;
  rendered.rerender(view());
  await waitFor(() => expect(read).toHaveBeenCalledTimes(3));
});

test('opening details before any request starts following once the first real request appears', async () => {
  projection = { ...session(), turns: [] };
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  expect(read).not.toHaveBeenCalled();
  projection = session();
  rendered.rerender(view());
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
});

test('a delayed older request cannot overwrite the newest request in follow mode', async () => {
  const pending = deferred();
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
  read.mockReturnValueOnce(pending.promise);
  projection = session('step_2');
  rendered.rerender(view());
  projection = session('step_3');
  rendered.rerender(view());
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_3'));
  await act(async () => {
    pending.resolve(inspection('step_2'));
  });
  expect(screen.getByLabelText('详情步骤').textContent).toBe('step_3');
});

test('changing sessions hides old content and rejects the old session’s pending response', async () => {
  const pending = deferred();
  read.mockReturnValueOnce(pending.promise);
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  projection = session('step_other', usage(100), 'other');
  rendered.rerender(view('other'));
  await act(async () => {
    pending.resolve(inspection('step_1'));
  });
  expect(screen.getByLabelText('详情步骤').textContent).toBe('empty');
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_other'));
});

test('a runtime replacement invalidates an older pending read and reloads the latest request', async () => {
  const pending = deferred();
  read.mockReturnValueOnce(pending.promise);
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  projection = session('step_new_runtime', usage(100), 'session', 'runtime_2');
  rendered.rerender(view());
  await waitFor(() =>
    expect(screen.getByLabelText('详情步骤').textContent).toBe('step_new_runtime'),
  );
  await act(async () => {
    pending.resolve(inspection('step_1'));
  });
  expect(screen.getByLabelText('详情步骤').textContent).toBe('step_new_runtime');
});

test('a failed automatic refresh retains the prior inspection and a later request can recover', async () => {
  const rendered = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
  read.mockResolvedValueOnce({
    ok: false,
    error: { code: 'inspection.failed', message: '读取失败。' },
  });
  projection = session('step_2');
  rendered.rerender(view());
  expect((await screen.findByRole('alert')).textContent).toContain('读取失败');
  expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1');
  projection = session('step_3');
  rendered.rerender(view());
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_3'));
  expect(screen.queryByRole('alert')).toBeNull();
});

test('a response naming another session or step is refused instead of being displayed', async () => {
  read.mockResolvedValueOnce({ ok: true, value: inspection('step_other', 'session_other') });
  render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  expect((await screen.findByRole('alert')).textContent).toContain('与当前请求不一致');
  expect(screen.getByLabelText('详情步骤').textContent).toBe('empty');
});

// The
// conversation view is rebuilt when the user goes to another conversation or to Settings and back;
// the details tab is still open, so it still shows its step.
test('open details keep their step when the conversation view is rebuilt', async () => {
  const first = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
  first.unmount();
  render(view());
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
});

test('an open details tab with no step chosen yet shows the latest request', async () => {
  const first = render(view());
  fireEvent.click(screen.getByRole('button', { name: '打开上下文详情' }));
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_1'));
  first.unmount();
  projection = session('step_2');
  render(view());
  await waitFor(() => expect(screen.getByLabelText('详情步骤').textContent).toBe('step_2'));
});
