// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { execute } from './actions.js';
import { command, S, setup } from './executor-fakes.js';

const open = command({ kind: 'open', url: 'http://127.0.0.1:9/game' });
const look = command({ kind: 'snapshot', max_chars: 12_000 });
const click = command(
  { kind: 'click', element: 'e1', x: null, y: null, button: 'left', double: false },
  '127.0.0.1',
);

/**
 * What the model learns of the user's takeover of its page.
 */
describe("the user's takeover of the model's page, as the model learns of it", () => {
  it('while the user has the page, says how to wait for it', async () => {
    const { deps, pages, tools } = setup();
    await execute(deps, open);
    pages.setUserControl(S, 'model', true);
    expect((await execute(deps, click)).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('browser.request_user'),
    });
    expect((await execute(deps, look)).outcome).toMatchObject({
      outcome: 'completed',
      result: { notice: expect.stringContaining('已接管') },
    });
    expect(tools.methods()).not.toContain('Input.dispatchMouseEvent');
  });

  it('after the user hands the page back, acts only once the model has looked at it again', async () => {
    const { deps, pages, tools } = setup();
    await execute(deps, open);
    pages.setUserControl(S, 'model', true);
    pages.setUserControl(S, 'model', false);
    expect((await execute(deps, click)).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('browser.snapshot'),
    });
    expect(tools.methods()).not.toContain('Input.dispatchMouseEvent');
    expect((await execute(deps, look)).outcome).toMatchObject({
      outcome: 'completed',
      result: { notice: expect.stringContaining('已经交还') },
    });
    expect((await execute(deps, click)).outcome.outcome).toBe('completed');
    // Seen once, the page is the model's again.
    expect((await execute(deps, look)).outcome).toMatchObject({ result: { notice: null } });
  });

  it("the user's own navigation of the page is told the same way", async () => {
    const { deps, pages, tools } = setup();
    await execute(deps, open);
    expect(pages.navigate(S, 'model', 'http://127.0.0.1:9/scores')).toBeUndefined();
    expect((await execute(deps, click)).outcome).toMatchObject({
      outcome: 'refused',
      message: expect.stringContaining('打开了网址'),
    });
    expect(tools.methods()).not.toContain('Input.dispatchMouseEvent');
    expect((await execute(deps, look)).outcome).toMatchObject({
      result: { url: 'http://127.0.0.1:9/scores', notice: expect.stringContaining('打开了网址') },
    });
    expect((await execute(deps, click)).outcome.outcome).toBe('completed');
    // The model's own navigation is no news to it.
    pages.history(S, 'model', 'reload', true);
    expect((await execute(deps, click)).outcome.outcome).toBe('completed');
  });

  it("a request for help, answered, is the model's look at the page", async () => {
    const { deps, pages } = setup({
      assist: async () => {
        pages.setAssist(S, 'model', undefined);
        return 'done';
      },
    });
    await execute(deps, open);
    pages.setUserControl(S, 'model', true);
    const asked = await execute(
      deps,
      command({ kind: 'request_user', reason: '请操作完后点完成', timeout_ms: 60_000 }),
    );
    expect(asked.outcome).toMatchObject({ outcome: 'completed', result: { outcome: 'done' } });
    // Its page is a new snapshot, with refs of its own.
    const at = command(
      { kind: 'click', element: null, x: 10, y: 10, button: 'left', double: false },
      '127.0.0.1',
    );
    expect((await execute(deps, at)).outcome.outcome).toBe('completed');
  });
});
