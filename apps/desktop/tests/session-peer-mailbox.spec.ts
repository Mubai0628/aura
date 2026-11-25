import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';

// eslint-disable-next-line no-empty-pattern -- the fixture opens real Electron and Core on private test data.
test('peer review stays with its own ordinary session after an Electron and Core restart', async ({}, info) => {
  test.setTimeout(120_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-peer-mailbox-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  await info.attach('isolated-data-root', { body: root, contentType: 'text/plain' });
  const first = await launch(root);
  const sessions = await (async () => {
    try {
      return await first.page.evaluate(async (selected_root) => {
        const api = window.aura.v1;
        const value = <T>(result: { ok: true; value: T } | { ok: false; error: unknown }) => {
          if (!result.ok) throw new Error(JSON.stringify(result.error));
          return result.value;
        };
        const { workspace } = value(
          await api.createWorkspace({ selected_root, display_name: '协作夹具' }),
        );
        const a = value(
          await api.createSession({ workspace_id: workspace.workspace_id, title: '审批侧' }),
        );
        const b = value(
          await api.createSession({ workspace_id: workspace.workspace_id, title: '独立会话' }),
        );
        const before = value(await api.readPeerMailbox({ session_id: a.session_id }));
        const after = value(
          await api.managePeerMailbox({
            session_id: a.session_id,
            action: {
              kind: 'set_review',
              required: true,
              expected_revision: before.review_policy.revision,
            },
          }),
        );
        return { a: a.session_id, b: b.session_id, before, after };
      }, workspace);
    } finally {
      await first.app.close();
    }
  })();
  expect(sessions.before).toMatchObject({
    budget_default: { limit: 32 },
    review_policy: { required: false },
    entries: [],
  });
  expect(sessions.after.review_policy).toMatchObject({ required: true });

  const reopened = await launch(root);
  try {
    const results = await reopened.page.evaluate(
      async ({ a, b }) => {
        const api = window.aura.v1;
        return Promise.all([
          api.readPeerMailbox({ session_id: a }),
          api.readPeerMailbox({ session_id: b }),
        ]);
      },
      { a: sessions.a, b: sessions.b },
    );
    expect(results[0]).toMatchObject({
      ok: true,
      value: { review_policy: { required: true }, entries: [] },
    });
    expect(results[1]).toMatchObject({
      ok: true,
      value: { review_policy: { required: false }, entries: [] },
    });
  } finally {
    await reopened.app.close();
  }
});
