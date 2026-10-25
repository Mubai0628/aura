import { expect, test } from '@playwright/test';
import { mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { writeProvider } from './s06-file-write-provider.mjs';
import { armReadOnlyWrite } from './s06/setup';
import { answer, changeMode, fileGrants, finished, hash, launch, submit } from './s06/harness';

// eslint-disable-next-line no-empty-pattern -- real Electron, preload save route and production Core.
test('S06 R5 Aura editor publication keeps Session approval through the next Agent write', async ({}, info) => {
  test.setTimeout(240_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-s06-editor-'));
  const workspace = join(root, 'workspace');
  await mkdir(workspace);
  const target = join(workspace, 'editor.txt');
  const provider = await writeProvider({ path: 'editor.txt', text: 'Agent before editor' });
  let current = await launch(root);
  try {
    const setup = await armReadOnlyWrite(current.page, workspace, provider.url, {
      title: 'S06 editor publication',
      text: '写入 editor.txt',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const workspaceId = setup.workspaceId as string;
    await answer(current.page, session, 'allow_session');
    await finished(current.page, session);
    const original = (await fileGrants(current.page, session))[0]!;
    await changeMode(current.page, session, 'workspace_read_write');
    const saved = await current.page.evaluate(
      async ({ session_id, workspace_id }) => {
        const path = 'editor.txt';
        const opened = await window.aura.v1.openWorkspaceBuffer({
          workspace_id,
          session_id,
          path,
          writable: true,
        });
        if (!opened.ok) throw new Error(JSON.stringify(opened.error));
        const buffer = opened.value;
        if (!buffer.reservation_id)
          throw new Error('The real editor save requires its reservation.');
        const edited = await window.aura.v1.editWorkspaceBuffer({
          workspace_id,
          session_id,
          path,
          reservation_id: buffer.reservation_id,
          reservation_epoch: buffer.reservation_epoch,
          buffer_revision: String(BigInt(buffer.buffer_revision) + 1n),
          base_core_revision: buffer.core_acknowledged_revision,
          content: 'Aura editor saved',
          active_editing: false,
          ime_composition_active: false,
        });
        if (!edited.ok) throw new Error(JSON.stringify(edited.error));
        return window.aura.v1.saveWorkspaceBuffer({
          workspace_id,
          session_id,
          path,
          reservation_id: buffer.reservation_id,
          reservation_epoch: edited.value.status.reservation_epoch,
          expected_buffer_revision: edited.value.status.buffer_revision,
        });
      },
      { session_id: session, workspace_id: workspaceId },
    );
    expect(saved.ok).toBe(true);
    expect(await readFile(target, 'utf8')).toBe('Aura editor saved');
    const followed = await fileGrants(current.page, session);
    expect(followed).toHaveLength(1);
    expect(followed[0]!.grant_id).toBe(original.grant_id);
    expect(followed[0]!.availability).toBe('in_force');
    expect(followed[0]!.scope.targets[0]!.identity_key).not.toBe(
      original.scope.targets[0]!.identity_key,
    );
    // Restart closes the editor's eligibility while preserving the saved approval in the real store.
    await current.app.close();
    current = await launch(root);
    await changeMode(current.page, session, 'read_only');
    provider.setAction('fs.write', {
      path: 'editor.txt',
      text: 'Agent after editor',
      expected: {
        expected_kind: 'file_hash',
        hash: await hash(current.page, workspaceId, session, 'editor.txt'),
      },
    });
    await submit(current.page, session, '继续写同一文件');
    await finished(current.page, session);
    expect(await readFile(target, 'utf8')).toBe('Agent after editor');
    expect((await fileGrants(current.page, session))[0]!.availability).toBe('in_force');
    expect(provider.errors).toEqual([]);
    await info.attach('production-root', { body: root, contentType: 'text/plain' });
  } finally {
    await current.app.close();
    await provider.close();
  }
});
