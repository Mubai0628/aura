/* global window, getComputedStyle */
import assert from 'node:assert/strict';
import { join } from 'node:path';
import {
  api,
  harness,
  sendNew,
  sendNext,
  completed,
  configureFixture,
  fileReadFacts,
  observeIncremental,
  poll,
} from './r10-session-harness.mjs';
import { fixtureProvider } from './r10-session-provider.mjs';

export async function runSessionAcceptance({ label, noTool = false, long = true }) {
  const provider = await fixtureProvider({ noTool, long });
  let h;
  let failure;
  try {
    h = await harness(label);
    await configureFixture(h.page, provider.url);
    const session = await sendNew(h, 'R10 test model', '读取目录及文件并报告结果');
    await observeIncremental(h.page, session);
    await h.page.locator('.agent-reasoning-trace').first().waitFor();
    await h.page.locator('.agent-reasoning-trace summary').first().click();
    await h.page.screenshot({ path: join(h.output, 'thinking.png') });
    const view = await completed(h, session);
    const turn = view.turns.at(-1);
    const process = h.page.locator(`[data-turn-id="${turn.turn.turn_task_id}"] .agent-process`);
    const toggle = process.locator('.agent-process-toggle');
    assert.equal(await toggle.getAttribute('aria-expanded'), 'false');
    await toggle.focus();
    await h.page.keyboard.press('Space');
    assert.equal(await toggle.getAttribute('aria-expanded'), 'true');
    const activity = noTool ? null : await fileReadFacts(h, session, turn);
    // The surrounding session activity card shares this class. Only actual
    // calls in this turn's process belong in the rendered-call assertion.
    const chips = process.locator('.agent-steps .agent-tool-activity');
    if (activity) {
      assert.equal(activity.calls.length, long ? 27 : 3);
      assert.ok(activity.calls.every((c) => c.summary.status === 'finalized'));
      assert.ok(activity.calls.every((c) => c.summary.outcome?.type === 'succeeded'));
      const visible = activity.calls.filter((call) => !call.display.internal);
      assert.equal(visible.length, long ? 26 : 2);
      try {
        await poll(
          () => chips.count(),
          (n) => n === visible.length,
          20_000,
          `all ${visible.length} calls in this turn, including calls older than 20`,
        );
        assert.equal(await process.locator('.agent-tool-pending').count(), 0);
        assert.deepEqual(
          (await chips.locator('.agent-tool-title').allTextContents()).sort(),
          visible.map((call) => call.display.title).sort(),
        );
        assert.deepEqual(
          (await chips.locator('.agent-file-label').allTextContents()).sort(),
          visible
            .map((call) => call.display.target)
            .filter(Boolean)
            .sort(),
        );
        assert.equal(
          await chips.locator('.agent-tool-state', { hasText: /^成功$/u }).count(),
          visible.length,
        );
      } finally {
        h.record('turn-scoped tool rendering counts', {
          persistedCalls: activity.calls.length,
          expectedProcessCalls: visible.length,
          renderedProcessCalls: await chips.count(),
          pendingProcessCalls: await process.locator('.agent-tool-pending').count(),
          pageWideMatches: await h.page.locator('.agent-tool-activity').count(),
        });
      }
      await chips.locator('summary').first().click();
      await h.page.screenshot({ path: join(h.output, 'tool-chip.png') });
    } else {
      await poll(
        () => process.locator('.agent-tool-pending').count(),
        (n) => n === 0,
      );
      assert.equal(await chips.count(), 0, 'internal completion stays in inspection details');
    }
    assert.equal(await h.page.locator('.agent-process-step .agent-turn-status').count(), 0);
    assert.equal(await h.page.getByText('上下文压缩', { exact: true }).count(), 0);
    const changes = await h.page.evaluate(() => window.__r10VisibleChanges);
    assert.ok(changes.length >= 2, 'real projection displayed incremental text');
    const style = await h.page
      .locator('.agent-reasoning')
      .first()
      .evaluate((e) => ({
        color: getComputedStyle(e).color,
        border: getComputedStyle(e).borderLeftWidth,
      }));
    h.record('single status, keyboard process disclosure, incremental output and bounded chips', {
      calls: activity?.calls.length ?? 0,
      visibleChanges: changes.length,
      style,
    });
    await sendNext(h, '第二条普通消息');
    await completed(h, session, 2);
    const registry = await api(h.page, 'getWorkspaceSessions');
    assert.equal(
      registry.payload.sessions.find((s) => s.session_id === session).title,
      '读取目录及',
    );
    assert.equal(await h.page.locator('.agent-turn-status').count(), 2);
    // The first turn's manual disclosure choice survives later projections.
    assert.equal(
      await h.page.locator('.agent-process-toggle').first().getAttribute('aria-expanded'),
      'true',
    );
    const stable = provider.requests[0].stable;
    assert.ok(
      provider.requests.every((r) => JSON.stringify(r.stable) === JSON.stringify(stable)),
      'stable system prefix across steps and turns',
    );
    assert.equal(provider.errors.length, 0, provider.errors.join('\n'));
    h.record('second ordinary send auto-starts, title stays fixed, stable request prefix', {
      requests: provider.requests.length,
      completedTurns: 2,
    });
    await h.page.emulateMedia({ reducedMotion: 'reduce' });
    await h.page.screenshot({ path: join(h.output, 'process.png'), fullPage: true });
  } catch (error) {
    failure = error;
  } finally {
    await provider.close();
  }
  if (h) await h.finish(failure);
  else if (failure) throw failure;
}
