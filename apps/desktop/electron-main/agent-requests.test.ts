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

describe('reads a busy store did not answer in time', () => {
  const timedOut = () =>
    Object.assign(new Error('no reply in time'), {
      envelope: { code: 'ipc.request_timeout', terminal: 'failed' },
    });

  /** A transport whose first `failures` requests time out; it counts every request. */
  function transportFailing(failures: number) {
    let sent = 0;
    const transport = {
      request: () => {
        sent += 1;
        return sent <= failures ? Promise.reject(timedOut()) : Promise.resolve({ ok: true });
      },
    } as unknown as CoreTransport;
    return { transport, sent: () => sent };
  }

  const viaTransport =
    (transport: CoreTransport): Parameters<typeof requestAgentCommand>[2] =>
    async (options) =>
      options.send(transport) as never;

  it('a read is asked again until it is answered', async () => {
    const { transport, sent } = transportFailing(2);
    await expect(
      requestAgentCommand(
        'agent.history',
        {} as AgentMainParams['agent.history'],
        viaTransport(transport),
      ),
    ).resolves.toEqual({ ok: true });
    expect(sent()).toBe(3);
  });

  it('a command that changes something is never sent again', async () => {
    const { transport, sent } = transportFailing(1);
    await expect(
      requestAgentCommand(
        'agent.submit',
        {} as AgentMainParams['agent.submit'],
        viaTransport(transport),
      ),
    ).rejects.toThrow('no reply in time');
    expect(sent()).toBe(1);
  });
});
