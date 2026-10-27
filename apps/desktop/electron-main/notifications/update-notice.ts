import type { AppUpdateState } from '../../shared/app-updates.js';
import { UPDATES_IN_FRONT } from '../../shared/notifications.js';

export interface UpdateNoticeDeps {
  /** Whether 设置 → 通知 lets a new version be announced. */
  allowed(): Promise<boolean>;
  /** The version announced last, so each version is announced once. */
  announced(): string | null;
  remember(version: string): Promise<void>;
  inFront(key: string): boolean;
  show(title: string, body: string): void;
  /** The red dot on the taskbar button, until 关于与更新 has been in front. */
  attention(key: string): void;
}

/**
 * A new version, announced once: a Windows notification and the red dot, unless 关于与更新 is in
 * front already.
 */
export async function announceUpdate(state: AppUpdateState, deps: UpdateNoticeDeps): Promise<void> {
  const version = state.latest?.version;
  if (state.phase !== 'available' || version === undefined) return;
  if (!(await deps.allowed()) || deps.announced() === version) return;
  await deps.remember(version);
  if (deps.inFront(UPDATES_IN_FRONT)) return;
  deps.show('Aura 有新版本', `${version} 可以下载更新`);
  deps.attention(UPDATES_IN_FRONT);
}
