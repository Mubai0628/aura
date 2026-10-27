// @vitest-environment node
import { expect, test, vi } from 'vitest';

import type { AppUpdateState } from '../../shared/app-updates.js';
import { UNAVAILABLE_UPDATE_STATE } from '../../shared/app-updates.js';
import { UPDATES_IN_FRONT } from '../../shared/notifications.js';
import { announceUpdate, type UpdateNoticeDeps } from './update-notice.js';

const available = (version: string): AppUpdateState => ({
  ...UNAVAILABLE_UPDATE_STATE,
  available: true,
  phase: 'available',
  latest: { version, published_at: null, size: 1, notes: '' },
});

function deps(options: { allowed?: boolean; inFront?: boolean } = {}) {
  let announced: string | null = null;
  const calls = {
    show: vi.fn<UpdateNoticeDeps['show']>(),
    attention: vi.fn<UpdateNoticeDeps['attention']>(),
  };
  const value: UpdateNoticeDeps = {
    allowed: async () => options.allowed ?? true,
    announced: () => announced,
    remember: async (version) => {
      announced = version;
    },
    inFront: () => options.inFront ?? false,
    ...calls,
  };
  return { value, calls };
}

test('a new version is announced once, with the red dot until 关于与更新 has been in front', async () => {
  const { value, calls } = deps();
  await announceUpdate(available('0.1.7'), value);
  expect(calls.show).toHaveBeenCalledWith('Aura 有新版本', '0.1.7 可以下载更新');
  expect(calls.attention).toHaveBeenCalledWith(UPDATES_IN_FRONT);
  await announceUpdate(available('0.1.7'), value);
  expect(calls.show).toHaveBeenCalledTimes(1);
  await announceUpdate(available('0.1.8'), value);
  expect(calls.show).toHaveBeenCalledTimes(2);
});

test('nothing is said while the switch is off, 关于与更新 is in front or no version waits', async () => {
  const off = deps({ allowed: false });
  await announceUpdate(available('0.1.7'), off.value);
  expect(off.calls.show).not.toHaveBeenCalled();
  const looking = deps({ inFront: true });
  await announceUpdate(available('0.1.7'), looking.value);
  expect(looking.calls.show).not.toHaveBeenCalled();
  expect(looking.calls.attention).not.toHaveBeenCalled();
  const none = deps();
  await announceUpdate({ ...available('0.1.7'), phase: 'up_to_date', latest: null }, none.value);
  expect(none.calls.show).not.toHaveBeenCalled();
});
