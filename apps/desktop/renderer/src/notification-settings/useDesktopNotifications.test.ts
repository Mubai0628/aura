import { expect, test } from 'vitest';
import { UPDATES_IN_FRONT } from '../../../shared/notifications';
import { frontOf } from './useDesktopNotifications';

test('the conversation shown is in front, 关于与更新 while it shows, otherwise nothing', () => {
  const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
  expect(frontOf({ screen: 'workspace', section: 'about', session })).toBe(session);
  expect(frontOf({ screen: 'workspace', section: 'about', session: null })).toBeNull();
  expect(frontOf({ screen: 'settings', section: 'about', session })).toBe(UPDATES_IN_FRONT);
  expect(frontOf({ screen: 'settings', section: 'models', session })).toBeNull();
});
