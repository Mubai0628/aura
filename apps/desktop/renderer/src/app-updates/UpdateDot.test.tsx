import { cleanup, render } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { UNAVAILABLE_UPDATE_STATE, type AppUpdateState } from '../../../shared/app-updates';
import { UpdateDot } from './UpdateDot';

let current: AppUpdateState | undefined;
vi.mock('./useAppUpdates', () => ({
  useAppUpdates: () => ({
    state: current,
    error: undefined,
    pending: undefined,
    send: async () => true,
  }),
}));

beforeEach(() => {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { appUpdate: async () => UNAVAILABLE_UPDATE_STATE } },
  });
});

afterEach(() => {
  cleanup();
  current = undefined;
  Reflect.deleteProperty(window, 'aura');
});

const release = { version: '0.1.7', published_at: null, size: 1, notes: '' };

test('the dot shows while a newer version waits and goes once there is none', () => {
  current = { ...UNAVAILABLE_UPDATE_STATE, available: true, phase: 'available', latest: release };
  const { container, rerender } = render(<UpdateDot />);
  expect(container.querySelector('.update-dot')).not.toBeNull();
  current = { ...current, phase: 'error', error: '更新没能安装' };
  rerender(<UpdateDot />);
  expect(container.querySelector('.update-dot')).not.toBeNull();
  current = { ...current, phase: 'up_to_date', latest: null, error: null };
  rerender(<UpdateDot />);
  expect(container.querySelector('.update-dot')).toBeNull();
});

test('no dot in a run that cannot update, before the state is known, or without the bridge', () => {
  const { container, rerender } = render(<UpdateDot />);
  expect(container.querySelector('.update-dot')).toBeNull();
  current = { ...UNAVAILABLE_UPDATE_STATE, latest: release };
  rerender(<UpdateDot />);
  expect(container.querySelector('.update-dot')).toBeNull();
  current = { ...UNAVAILABLE_UPDATE_STATE, available: true, latest: release };
  Reflect.deleteProperty(window, 'aura');
  rerender(<UpdateDot />);
  expect(container.querySelector('.update-dot')).toBeNull();
});
