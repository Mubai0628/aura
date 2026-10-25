import { expect, test } from '@playwright/test';
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';
import { promisify } from 'node:util';
import { launch, pending, answer, finished, submit } from './s06/harness';
import { armReadOnlyWrite } from './s06/setup';
import { networkProvider } from './s07/provider';

// eslint-disable-next-line no-empty-pattern -- Electron owns the page; no browser download.
test('Git push session consent and exact leases cross the real desktop approval and revocation paths', async ({}, info) => {
  test.setTimeout(180_000);
  const root = await mkdtemp(join(tmpdir(), 'aura-git-test-remote-desktop-'));
  const workspace = join(root, 'workspace'),
    bare = join(root, 'remote.git');
  await mkdir(workspace);
  await mkdir(bare);
  const global = join(root, 'global.cfg'),
    system = join(root, 'system.cfg'),
    profile = join(root, 'profile.json');
  await writeFile(
    global,
    '[credential]\nhelper =\n[user]\nname = Fixture\nemail = fixture@example.invalid\n[commit]\ngpgsign = false\n',
  );
  await writeFile(system, '');
  const image = process.env.AURA_GIT_TEST_BINARY ?? 'C:/Program Files/Git/cmd/git.exe';
  await writeFile(
    profile,
    JSON.stringify({ global_config: global, system_config: system, git_path: image }),
  );
  const environment = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.toUpperCase().startsWith('GIT_')),
  );
  const git = (cwd: string, args: string[]) =>
    promisify(execFile)(image, args, {
      cwd,
      windowsHide: true,
      env: {
        ...environment,
        HOME: root,
        XDG_CONFIG_HOME: root,
        GIT_CONFIG_GLOBAL: global,
        GIT_CONFIG_SYSTEM: system,
        GIT_CONFIG_NOSYSTEM: '1',
        GIT_TERMINAL_PROMPT: '0',
        HTTP_PROXY: '',
        HTTPS_PROXY: '',
        ALL_PROXY: '',
        NO_PROXY: '*',
      },
    });
  await git(bare, ['init', '--bare', '--initial-branch=main']);
  await git(workspace, ['init', '--initial-branch=main']);
  await writeFile(join(workspace, 'file.txt'), 'desktop remote fixture\n');
  await git(workspace, ['add', 'file.txt']);
  await git(workspace, ['commit', '-m', 'desktop remote']);
  await git(workspace, ['remote', 'add', 'origin', pathToFileURL(bare).href]);
  const head = (await git(workspace, ['rev-parse', 'HEAD'])).stdout.trim();
  const provider = await networkProvider();
  const args = { remote: 'origin', remote_branch: 'review' };
  provider.setAction('git.push', args);
  const { app, page } = await launch(root, { AURA_GIT_TEST_PROFILE: profile });
  try {
    const setup = await armReadOnlyWrite(page, workspace, provider.url, {
      title: 'L03 remote Git',
      text: '将当前分支推送到 origin/review。',
    });
    expect(setup.error).toBeUndefined();
    const session = setup.sessionId as string;
    const proposal = await pending(page, session);
    expect(proposal.backend.class).toBe('native');
    expect(proposal.operation.content.capabilities).toEqual(['git_remote_write']);
    // The card shows the model's explanation; the facts are under 查看详情.
    await page.locator('.agent-approval').getByText('查看详情').click();
    await expect(page.locator('.agent-approval').getByText('main → review')).toBeVisible();
    await expect(page.locator('.agent-approval').getByText(/只覆盖这个物理仓库/)).toBeVisible();
    expect((await git(bare, ['for-each-ref'])).stdout).toBe('');
    await answer(page, session, 'allow_session');
    await finished(page, session);
    expect((await git(bare, ['rev-parse', 'refs/heads/review'])).stdout.trim()).toBe(head);
    const activity = await page.evaluate(async (session_id) => {
      const result = await window.aura.v1.readAgentToolActivity({
        session_id,
        before_call_id: null,
        call_ids: null,
        limit: 100,
      });
      if (!result.ok) throw Error(result.error.message);
      return result.value.calls.map((call) => call.display.summary);
    }, session);
    expect(
      activity.some(
        (summary) =>
          summary.includes('推送 main → origin/review') &&
          summary.includes('Aura 不隔离它的文件写入'),
      ),
    ).toBe(true);
    const grants = await page.evaluate(async (session_id) => {
      const result = await window.aura.v1.listNetworkSessionConsents({
        session_id,
        include_inactive: false,
      });
      if (!result.ok) throw Error(result.error.message);
      return result.value;
    }, session);
    const grant = grants.find((value) => value.scope.caller === 'git');
    expect(grant?.scope.destinations).toMatchObject({
      git: { target: { remote_branch: 'review' } },
    });
    await submit(page, session, '再次推送相同远端分支，使用本会话批准。');
    await finished(page, session);
    provider.setAction('git.push', { ...args, force_with_lease: head });
    await submit(page, session, '使用明确的期望提交号推送，等待一次批准。');
    const force = await pending(page, session);
    expect(force.approval.allowed_decisions).toEqual(['allow_once', 'deny']);
    await page.locator('.agent-approval').getByText('查看详情').click();
    await expect(page.locator('.agent-approval').getByText(head, { exact: true })).toBeVisible();
    await answer(page, session, 'allow_once');
    await finished(page, session);
    const revoked = await page.evaluate(
      async ({ session_id, grant_id, expected_revision }) =>
        window.aura.v1.revokeNetworkSessionConsent({ session_id, grant_id, expected_revision }),
      { session_id: session, grant_id: grant!.grant_id, expected_revision: grant!.grant_revision },
    );
    expect(revoked.ok).toBe(true);
    provider.setAction('git.push', args);
    await submit(page, session, '批准撤销后再推送，应重新确认。');
    await pending(page, session);
    await answer(page, session, 'deny');
    await finished(page, session);
    expect(provider.errors).toEqual([]);
    await info.attach('temporary-root', { body: root, contentType: 'text/plain' });
  } finally {
    await app.close();
    await provider.close();
  }
});
