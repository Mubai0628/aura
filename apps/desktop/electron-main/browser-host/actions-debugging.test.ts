// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { execute } from './actions.js';
import type { AssistOutcome } from './assist.js';
import { command, S, setup } from './executor-fakes.js';
import { parsedRules } from './site-rules.js';

const POLICY = { enabled: true, allowed: [], blocked: parsedRules(['blocked.example']) };
const open = { kind: 'open', url: 'http://127.0.0.1:9/game' } as const;

describe("the model's request for the user's help", () => {
  it("leaves the page to the user, then answers with the user's word and a fresh snapshot", async () => {
    let asked: [string, string, number] | undefined;
    let activeWhileWaiting: boolean | undefined;
    const { deps, pages, devtools } = setup({
      assist: async (_session, commandId, reason, timeoutMs) => {
        asked = [commandId, reason, timeoutMs];
        activeWhileWaiting = pages.state(S, 'model')?.agentActive;
        return 'done';
      },
    });
    await execute(deps, command(open));
    const before = devtools.refs.refFor(50);
    const result = await execute(
      deps,
      command({ kind: 'request_user', reason: '请登录测试账号', timeout_ms: 180_000 }),
    );
    expect(asked).toEqual(['bcm-1', '请登录测试账号', 180_000]);
    expect(activeWhileWaiting).toBe(false);
    expect(result.outcome).toMatchObject({
      outcome: 'completed',
      result: { outcome: 'done', page: { url: 'http://127.0.0.1:9/game' } },
    });
    // Refs from before the user acted are gone; the page has new ones.
    expect(devtools.refs.node(before)).not.toBe(50);
    expect(JSON.stringify(result.outcome)).not.toContain(`[ref=${before}]`);
  });

  it('says how it ended: declined, timed out, the page closed, or Core stopped waiting', async () => {
    for (const ending of ['declined', 'timed_out', 'closed', 'withdrawn'] as const) {
      const { deps } = setup({ assist: async () => ending as AssistOutcome | 'withdrawn' });
      await execute(deps, command(open));
      const result = await execute(
        deps,
        command({ kind: 'request_user', reason: '请选择', timeout_ms: 30_000 }),
      );
      if (ending === 'withdrawn') expect(result.outcome.outcome).toBe('refused');
      else if (ending === 'closed')
        expect(result.outcome).toMatchObject({ result: { outcome: 'closed', page: null } });
      else expect(result.outcome).toMatchObject({ result: { outcome: ending } });
    }
  });
});

describe('page debugging', () => {
  it("is refused while the user's switch is off, whatever Core sent", async () => {
    const { deps, pages, tools } = setup();
    pages.setPolicy(POLICY);
    await execute(deps, command(open));
    for (const action of [
      { kind: 'evaluate', expression: 'document.cookie' } as const,
      { kind: 'network', failed_only: false, max_entries: 50 } as const,
    ]) {
      const result = await execute(deps, command(action, '127.0.0.1'));
      expect(result.outcome).toMatchObject({
        outcome: 'refused',
        message: expect.stringContaining('允许模型调试网页'),
      });
    }
    expect(tools.methods()).not.toContain('Runtime.evaluate');
  });

  it('runs a script on the page it was approved for and returns its value or what it threw', async () => {
    const { deps, pages, tools } = setup();
    pages.setPolicy({ ...POLICY, debugging: true });
    await execute(deps, command(open));
    tools.answers.set('Runtime.evaluate', (params) =>
      params.expression === 'boom()'
        ? { result: {}, exceptionDetails: { exception: { description: 'ReferenceError: boom' } } }
        : { result: { type: 'object', value: { score: 42, lives: 3 } } },
    );
    const value = await execute(
      deps,
      command({ kind: 'evaluate', expression: 'window.game' }, '127.0.0.1'),
    );
    expect(value.outcome).toMatchObject({
      outcome: 'completed',
      result: { value: '{"score":42,"lives":3}', value_truncated: false, exception: null },
    });
    const sent = tools.sent.find(([method]) => method === 'Runtime.evaluate')?.[1];
    expect(sent).toMatchObject({ awaitPromise: true, returnByValue: true, userGesture: false });
    const thrown = await execute(
      deps,
      command({ kind: 'evaluate', expression: 'boom()' }, '127.0.0.1'),
    );
    expect(thrown.outcome).toMatchObject({
      result: { value: null, exception: 'ReferenceError: boom' },
    });
    // On a page that moved away from what Core approved, nothing runs.
    tools.sent = [];
    const moved = await execute(
      deps,
      command({ kind: 'evaluate', expression: 'window.game' }, 'bank.example'),
    );
    expect(moved.outcome.outcome).toBe('refused');
    expect(tools.methods()).not.toContain('Runtime.evaluate');
    // Nor while the user has the page.
    pages.setUserControl(S, 'model', true);
    const taken = await execute(
      deps,
      command({ kind: 'evaluate', expression: 'window.game' }, '127.0.0.1'),
    );
    expect(taken.outcome).toMatchObject({ outcome: 'refused' });
  });

  it("lists the page's requests with their credentials hidden", async () => {
    const { deps, pages, tools } = setup();
    pages.setPolicy({ ...POLICY, debugging: true });
    await execute(deps, command(open));
    tools.emit('Network.requestWillBeSent', {
      requestId: '3',
      type: 'XHR',
      request: {
        method: 'GET',
        url: 'http://127.0.0.1:9/api?token=abc',
        headers: { Authorization: 'Bearer abc' },
      },
    });
    tools.emit('Network.responseReceived', {
      requestId: '3',
      response: { status: 500, mimeType: 'text/plain', headers: {} },
    });
    const result = await execute(
      deps,
      command({ kind: 'network', failed_only: true, max_entries: 50 }, '127.0.0.1'),
    );
    expect(result.outcome).toMatchObject({
      outcome: 'completed',
      result: {
        url: 'http://127.0.0.1:9/game',
        entries: [{ method: 'GET', kind: 'xhr', status: 500 }],
        truncated: false,
      },
    });
    expect(JSON.stringify(result.outcome)).not.toMatch(/abc/u);
  });
});
