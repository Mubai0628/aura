import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

import {
  DEFAULT_NOTIFICATION_PREFERENCES,
  readNotificationPreferences,
  type NotificationPreferences,
} from '../../shared/notifications.js';

interface Saved {
  preferences: NotificationPreferences;
  /** The newest version already announced, so a start does not announce it again. */
  announcedUpdate: string | null;
}

/**
 * The switches of 设置 → 通知, kept with this installation's desktop state. A missing or damaged
 * file reads as every switch on; nothing else depends on it.
 */
/** Why the file could not be written, by the file system's code. */
const WRITE_CAUSES: Readonly<Record<string, string>> = {
  ENOSPC: '磁盘已满',
  EPERM: '没有写入权限',
  EACCES: '没有写入权限',
  EBUSY: '文件正被其他程序占用',
};

const notWritten = (error: unknown): string => {
  const code = (error as { code?: unknown } | null)?.code;
  const cause =
    (typeof code === 'string' ? WRITE_CAUSES[code] : undefined) ?? '磁盘已满或没有写入权限';
  return `通知设置没能写入磁盘（${cause}），这次改动没有保存；腾出空间或检查 Aura 数据文件夹的权限后再切换一次。`;
};

export class NotificationPreferencesStore {
  readonly #path: string;
  #saved: Saved = { preferences: { ...DEFAULT_NOTIFICATION_PREFERENCES }, announcedUpdate: null };
  #loaded: Promise<void>;
  #writes: Promise<void> = Promise.resolve();

  constructor(namespaceDataRoot: string) {
    this.#path = resolve(namespaceDataRoot, 'desktop/notifications.v1.json');
    this.#loaded = this.#load();
  }

  get current(): NotificationPreferences {
    return this.#saved.preferences;
  }

  get announcedUpdate(): string | null {
    return this.#saved.announcedUpdate;
  }

  async read(): Promise<NotificationPreferences> {
    await this.#loaded;
    return this.current;
  }

  /**
   * Keeps the switches; one whose write failed is undone in memory, so what is shown is what the
   * file holds, and the reason is given.
   */
  async save(preferences: NotificationPreferences): Promise<NotificationPreferences> {
    await this.#loaded;
    const before = this.#saved;
    const wanted = { ...this.#saved, preferences: readNotificationPreferences(preferences) };
    this.#saved = wanted;
    try {
      await this.#write();
    } catch (error) {
      if (this.#saved === wanted) this.#saved = before;
      throw new Error(notWritten(error), { cause: error });
    }
    return this.current;
  }

  async announced(version: string): Promise<void> {
    await this.#loaded;
    this.#saved = { ...this.#saved, announcedUpdate: version };
    await this.#write();
  }

  async #load(): Promise<void> {
    try {
      const value = JSON.parse(await readFile(this.#path, 'utf8')) as Record<string, unknown>;
      this.#saved = {
        preferences: readNotificationPreferences(value.preferences),
        announcedUpdate: typeof value.announced_update === 'string' ? value.announced_update : null,
      };
    } catch {
      // Every switch on, as for a new installation.
    }
  }

  /**
   * One write at a time, through a temporary file, so a crash leaves the old file or the new. A
   * write that failed does not hold up the next one.
   */
  #write(): Promise<void> {
    const saved = this.#saved;
    this.#writes = this.#writes
      .catch(() => undefined)
      .then(async () => {
        await mkdir(dirname(this.#path), { recursive: true });
        const temporary = `${this.#path}.${process.pid}.tmp`;
        await writeFile(
          temporary,
          JSON.stringify({
            schema_version: 1,
            preferences: saved.preferences,
            announced_update: saved.announcedUpdate,
          }),
        );
        await rename(temporary, this.#path);
      });
    return this.#writes;
  }
}
