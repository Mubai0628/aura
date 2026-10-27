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

  async save(preferences: NotificationPreferences): Promise<NotificationPreferences> {
    await this.#loaded;
    this.#saved = { ...this.#saved, preferences: readNotificationPreferences(preferences) };
    await this.#write();
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

  /** One write at a time, through a temporary file, so a crash leaves the old file or the new. */
  #write(): Promise<void> {
    const saved = this.#saved;
    this.#writes = this.#writes.then(async () => {
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
