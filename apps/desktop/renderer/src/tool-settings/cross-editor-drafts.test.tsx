import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { AgentToolList } from './AgentToolList';
import { ExecutionStrategy } from './ExecutionStrategy';
import { useAgentConfiguration } from './useAgentConfiguration';

/** Both editors of the shared configuration on one page, as the settings page
 *  renders them: they read one snapshot, but each keeps its own unsaved input. */
function Harness() {
  const configuration = useAgentConfiguration();
  return (
    <>
      <AgentToolList configuration={configuration} />
      <ExecutionStrategy configuration={configuration} />
    </>
  );
}

function setup() {
  let current = {
    revision: '1',
    max_dynamic_tool_schemas: 50,
    max_concurrent_tool_calls: 4,
    default_timeout_ms: 120_000,
    max_retries: 3,
    fixed_tool_ids: [] as string[],
    tools: [
      {
        tool_id: 'tool_review',
        name: 'fs.read',
        description: 'read',
        source: 'builtin',
        required: false,
        available: true,
        default_loaded: false,
      },
    ],
  };
  const configureAgentTools = vi.fn(async (request: Record<string, unknown>) => {
    if (request.expected_revision !== current.revision) {
      return { ok: false, error: { message: 'tools.configuration_revision_conflict' } };
    }
    current = { ...current, ...request, revision: String(Number(current.revision) + 1) };
    return { ok: true, value: current };
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentToolSettings: vi.fn(async () => ({ ok: true, value: current })),
        configureAgentTools,
      },
    },
  });
  render(<Harness />);
  return configureAgentTools;
}

afterEach(cleanup);

test('saving the tool list keeps an unsaved strategy draft', async () => {
  const configure = setup();
  const concurrency = (await screen.findByLabelText('普通工具全局并发')) as HTMLInputElement;
  fireEvent.change(concurrency, { target: { value: '8' } });
  fireEvent.click(screen.getByLabelText('fs.read 默认加载'));
  fireEvent.click(screen.getByRole('button', { name: '保存配置' }));
  await waitFor(() => expect(configure).toHaveBeenCalledTimes(1));
  await screen.findByText('工具配置已保存。');
  // The save published a new snapshot; it did not touch the other editor's
  // unsaved input, so the number the user typed is still there.
  expect((screen.getByLabelText('普通工具全局并发') as HTMLInputElement).value).toBe('8');
});

test('saving the strategy keeps an unsaved tool-selection draft', async () => {
  const configure = setup();
  const toggle = await screen.findByLabelText('fs.read 默认加载');
  fireEvent.click(toggle);
  fireEvent.change(screen.getByLabelText('普通工具全局并发'), { target: { value: '8' } });
  fireEvent.click(screen.getByRole('button', { name: '保存执行策略' }));
  await waitFor(() => expect(configure).toHaveBeenCalledTimes(1));
  await screen.findByText('执行策略已保存。');
  // The selection is still pending in its own editor and must not be cleared by
  // the snapshot the strategy save published.
  expect(screen.getByLabelText('fs.read 默认加载').getAttribute('aria-checked') === 'true').toBe(
    true,
  );
});
