import { describe, expect, it } from 'vitest';
import type { AgentMainCommand, AgentMainParams } from '../shared/generated/routes.js';
import { replyBudget, requestAgentCommand } from './agent-requests.js';
import type { CoreTransport } from './core-transport.js';

// Core waits for a run's real end for up to 45 s (`RUN_STOP_BUDGET`) before it answers these.
const CORE_RUN_STOP_BUDGET_MS = 45_000;

/** The reply budget Main gives one agent command, as the transport sees it. */
async function budgetOf(command: AgentMainCommand): Promise<number> {
  let seen: number | undefined;
  const transport = {
    request: (_method: string, _params: unknown, _priority?: number, timeout?: number) => {
      seen = timeout ?? 15_000;
      return Promise.resolve({});
    },
  } as unknown as CoreTransport;
  await requestAgentCommand(command, {} as AgentMainParams[typeof command], async (options) => {
    await options.send(transport);
    return undefined as never;
  });
  return seen ?? Number.NaN;
}

describe('agent request budgets', () => {
  // 停止 answered "停止未确认" while Core could still be stopping the run.
  it.each([
    'agent.cancel',
    'agent.mode.control',
    'agent.query.answer',
    'agent.approval.resolve',
    'agent.context.recheck',
    'agent.context.window.resolve',
  ] as const)('%s waits past the run stop budget', async (command) => {
    expect(await budgetOf(command)).toBeGreaterThan(CORE_RUN_STOP_BUDGET_MS + 5_000);
  });

  it('a permission mode change, which can stop running calls, waits past it too', () => {
    expect(replyBudget('permission.set_session_mode')).toBeGreaterThan(
      CORE_RUN_STOP_BUDGET_MS + 5_000,
    );
  });

  // Adding a large folder or importing a large plugin answered "未确认" after 15 s while Core
  // was still working.
  it.each([
    'agent.plugins.preview',
    'agent.plugins.import',
    'agent.skills.preview',
    'agent.skills.import',
  ] as const)('%s waits for Core as long as the work takes', async (command) => {
    expect(await budgetOf(command)).toBe(125_000);
  });

  it('adding or restoring a workspace waits past the 120 s bound Core sets itself', () => {
    expect(replyBudget('workspace.create')).toBe(125_000);
    expect(replyBudget('workspace.restore')).toBe(125_000);
  });

  it('every other request keeps the ordinary budget', async () => {
    expect(await budgetOf('agent.status')).toBe(15_000);
    expect(replyBudget('permission.get_session_mode')).toBe(15_000);
  });
});
