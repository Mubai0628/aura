import { mkdtemp, mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';

import { DEFAULT_NOTIFICATION_PREFERENCES } from '../../shared/notifications.js';
import { NotificationPreferencesStore } from './preferences.js';

const roots: string[] = [];
async function root() {
  const made = await mkdtemp(join(tmpdir(), 'aura-notification-preferences-'));
  roots.push(made);
  return made;
}

afterEach(async () => {
  await Promise.all(roots.splice(0).map((made) => rm(made, { recursive: true, force: true })));
});

describe('notification switches', () => {
  it('are all on for a new installation, and kept as the user sets them', async () => {
    const data = await root();
    const store = new NotificationPreferencesStore(data);
    expect(await store.read()).toEqual(DEFAULT_NOTIFICATION_PREFERENCES);
    const set = { ...DEFAULT_NOTIFICATION_PREFERENCES, completions: false };
    await store.save(set);
    await store.announced('0.1.5');
    const again = new NotificationPreferencesStore(data);
    expect(await again.read()).toEqual(set);
    expect(again.announcedUpdate).toBe('0.1.5');
    const file = JSON.parse(
      await readFile(join(data, 'desktop', 'notifications.v1.json'), 'utf8'),
    ) as Record<string, unknown>;
    expect(file.schema_version).toBe(1);
  });

  it('a write that failed is undone in memory, says why, and does not block the next', async () => {
    const data = await root();
    const store = new NotificationPreferencesStore(data);
    await store.read();
    // A file where the folder goes makes the write fail.
    await writeFile(join(data, 'desktop'), 'not a folder');
    const off = { ...DEFAULT_NOTIFICATION_PREFERENCES, completions: false };
    await expect(store.save(off)).rejects.toThrow(
      '通知设置没能写入磁盘（磁盘已满或没有写入权限），这次改动没有保存；腾出空间或检查 Aura 数据文件夹的权限后再切换一次。',
    );
    expect(store.current).toEqual(DEFAULT_NOTIFICATION_PREFERENCES);
    await rm(join(data, 'desktop'));
    await expect(store.save(off)).resolves.toEqual(off);
  });

  it('read a damaged file as every switch on', async () => {
    const data = await root();
    await mkdir(join(data, 'desktop'), { recursive: true });
    await writeFile(join(data, 'desktop', 'notifications.v1.json'), '{"preferences": [');
    expect(await new NotificationPreferencesStore(data).read()).toEqual(
      DEFAULT_NOTIFICATION_PREFERENCES,
    );
  });
});
