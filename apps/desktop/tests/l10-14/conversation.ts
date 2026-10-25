import { expect, type Locator, type Page } from '@playwright/test';

/** One turn of a conversation as the history pages give it, oldest first. */
export interface TurnFacts {
  id: string;
  state: string;
  imported: boolean;
  user: string;
}

/** Every turn of a conversation, paged ten at a time as the window reads them. */
export async function allTurns(page: Page, session: string): Promise<TurnFacts[]> {
  return page.evaluate(async (session_id) => {
    const turns: TurnFacts[] = [];
    let before: string | null = null;
    for (let pages = 0; pages < 20; pages += 1) {
      const result = await window.aura.v1.readAgentHistory({
        session_id,
        limit: 10,
        before_turn_task_id: before,
      });
      if (!result.ok) throw new Error(JSON.stringify(result.error));
      const page = result.value.turns.map((turn) => ({
        id: turn.turn.turn_task_id,
        state: turn.turn.state.state,
        imported: Boolean(turn.imported),
        user: turn.user_messages[0]?.text ?? '',
      }));
      turns.unshift(...page);
      before = result.value.next_before_turn_task_id ?? null;
      if (before === null) return turns;
    }
    throw new Error('more history pages than a test conversation has');
  }, session);
}

/** The conversation has `count` turns, the last one answered, and nothing runs. */
export async function answered(page: Page, session: string, count: number) {
  await expect
    .poll(
      async () => {
        const active = await page.evaluate(async (session_id) => {
          const result = await window.aura.v1.readAgentHistory({ session_id, limit: 1 });
          return result.ok ? Boolean(result.value.active) : true;
        }, session);
        if (active) return undefined;
        const turns = await allTurns(page, session);
        return turns.length === count ? turns.at(-1)?.state : undefined;
      },
      { timeout: 120_000 },
    )
    .toBe('completed');
}

export async function send(page: Page, session: string, text: string) {
  const sent = await page.evaluate(
    async ({ session_id, text }) =>
      window.aura.v1.submitAgentMessage({ session_id, text, destination: { lane: 'next_turn' } }),
    { session_id: session, text },
  );
  expect(sent.ok, JSON.stringify(sent)).toBe(true);
}

/** Opens a finished turn's process, its sections and the step titled `title`; returns the step. */
export async function openStep(turn: Locator, title: string): Promise<Locator> {
  // The window folds a turn once it sees it completed. Wait for that (its status line
  // becomes the toggle) before opening anything, or the fold closes what was just opened.
  const status = turn.locator('.agent-turn-toggle');
  await expect(status).toBeVisible();
  if ((await status.getAttribute('aria-expanded')) === 'false') await status.click();
  for (const section of await turn.locator('.agent-process-toggle').all())
    if ((await section.getAttribute('aria-expanded')) === 'false') await section.click();
  const step = turn
    .locator('details.agent-tool-activity')
    .filter({ has: turn.page().locator('.agent-tool-title', { hasText: title }) })
    .first();
  await expect(step).toBeVisible();
  if ((await step.getAttribute('open')) === null) await step.locator('summary').click();
  await expect(step.locator('.agent-tool-facts')).toBeVisible();
  return step;
}
