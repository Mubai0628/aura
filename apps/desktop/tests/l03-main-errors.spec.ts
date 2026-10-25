import { expect, test } from '@playwright/test';
import { mkdtemp, readdir, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { launch } from './s06/harness';

async function records(root: string) {
  const dir = join(root, 'Local/Aura/namespaces');
  const values: {
    event: string;
    fields: { message?: string; quitting?: boolean; pid?: number };
  }[] = [];
  for (const name of await readdir(dir).catch(() => [])) {
    const text = await readFile(join(dir, name, 'logs/main.jsonl'), 'utf8').catch(() => '');
    for (const line of text.split('\n').slice(0, -1).filter(Boolean)) values.push(JSON.parse(line));
  }
  return values;
}
function alive(pid: number) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}
for (const kind of ['uncaught_exception', 'unhandled_rejection'] as const) {
  for (const quitting of [false, true]) {
    test(`Main ${kind} ${quitting ? 'during quit logs and exits' : 'logs and notifies without a modal'}`, async () => {
      test.setTimeout(90_000);
      const root = await mkdtemp(join(tmpdir(), 'aura-l03-main-error-'));
      const marker = `temporary-${kind}-${quitting}`;
      const { app, page } = await launch(root);
      const ownMain = app.process();
      let ownCore: number | undefined;
      let exited = false;
      try {
        await expect
          .poll(async () =>
            (await records(root)).some(
              (record) => record.event === 'core_image' && record.fields.pid !== undefined,
            ),
          )
          .toBe(true);
        ownCore = (await records(root)).findLast((record) => record.event === 'core_image')!.fields
          .pid;
        const closed = new Promise<void>((resolve) => ownMain.once('exit', () => resolve()));
        await app.evaluate(
          ({ app }, { kind, quitting, marker }) => {
            const fail = () => {
              if (kind === 'uncaught_exception') throw new Error(marker);
              void Promise.reject(new Error(marker));
            };
            if (quitting) {
              // Return the diagnostic RPC before exercising native quit. A
              // correct immediate exit must not look like a lost RPC reply.
              setTimeout(() => {
                app.prependOnceListener('before-quit', () => process.nextTick(fail));
                app.quit();
              }, 50);
            } else setTimeout(fail, 0);
          },
          { kind, quitting, marker },
        );
        if (quitting) {
          await Promise.race([
            closed.then(() => {
              exited = true;
            }),
            new Promise((resolve) => setTimeout(resolve, 10_000)),
          ]);
          expect(
            exited,
            'uncaught errors after before-quit must never leave an Electron modal',
          ).toBe(true);
          await expect.poll(() => alive(ownCore!), { timeout: 5_000 }).toBe(false);
        } else {
          const alert = page.getByRole('alert').filter({ hasText: 'Aura 主进程发生错误' });
          await expect(alert).toBeVisible({ timeout: 8_000 });
          await expect(alert).toContainText('日志');
          await expect(page.getByText('Aura Core 已就绪')).toBeVisible();
          if (kind === 'uncaught_exception')
            await page.screenshot({
              path: test.info().outputPath('main-error.png'),
              animations: 'disabled',
            });
          await page.reload();
          await expect(alert).toBeVisible();
          await alert.getByRole('button', { name: '关闭消息' }).click();
          await expect(alert).toHaveCount(0);
        }
        await expect
          .poll(
            async () =>
              (await records(root)).filter(
                (record) =>
                  record.event === `main.${kind}` &&
                  record.fields.message === marker &&
                  record.fields.quitting === quitting,
              ).length,
          )
          .toBe(1);
        if (!quitting) {
          await app.close();
          exited = true;
        }
      } finally {
        // Kill only the exact temporary processes created by this fixture after a red baseline.
        if (!exited && ownMain.pid && alive(ownMain.pid)) ownMain.kill();
        if (ownCore && alive(ownCore)) process.kill(ownCore);
      }
    });
  }
}
