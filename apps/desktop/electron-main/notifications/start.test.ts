// @vitest-environment node
import { app } from 'electron';
import { beforeEach, expect, test, vi } from 'vitest';

import { registerNotificationIdentity } from './identity.js';
import { registerNotifications } from './index.js';
import { startNotifications } from './start.js';

vi.mock('electron', () => ({ app: { setAppUserModelId: vi.fn(), isPackaged: false } }));
vi.mock('./identity.js', async (original) => ({
  ...(await original<typeof import('./identity.js')>()),
  registerNotificationIdentity: vi.fn(async () => undefined),
}));
vi.mock('./index.js', () => ({
  registerNotifications: vi.fn(() => ({ updateState: vi.fn(), stop: vi.fn() })),
}));

const session_id = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

beforeEach(() => {
  vi.clearAllMocks();
});

const start = (core: () => unknown, log = vi.fn()) => {
  startNotifications({
    namespace: 'production',
    dataRoot: 'C:\\data',
    window: () => undefined,
    trust: () => undefined,
    core: core as never,
    log,
  });
  return vi.mocked(registerNotifications).mock.calls[0]![0];
};

test('notifications go under the installation identity, named for Windows only when installed', () => {
  expect(start(() => undefined).dataRoot).toBe('C:\\data');
  expect(app.setAppUserModelId).toHaveBeenCalledWith('Aura.production');
  expect(registerNotificationIdentity).not.toHaveBeenCalled();
  Object.assign(app, { isPackaged: true });
  try {
    start(() => undefined);
    expect(registerNotificationIdentity).toHaveBeenCalledWith('Aura.production');
  } finally {
    Object.assign(app, { isPackaged: false });
  }
});

test('each look reads the conversations from the Core running at that moment', async () => {
  let core: unknown = undefined;
  const log = vi.fn();
  const { readActivity, sessionTitle, log: passed } = start(() => core, log);
  await expect(readActivity({ ended_turns: [] })).rejects.toThrow('Aura Core is not running');
  expect(sessionTitle(session_id)).toBeUndefined();
  passed('notifications.activity_unavailable', {});
  expect(log).toHaveBeenCalledWith('notifications.activity_unavailable', {});

  const activity = { sessions: [] };
  const requestAgent = vi.fn(async () => activity);
  core = {
    requestAgent,
    workspaceSessionSnapshot: { payload: { sessions: [{ session_id, title: '写一个游戏' }] } },
  };
  await expect(readActivity({ ended_turns: ['trn_1'] })).resolves.toBe(activity);
  expect(requestAgent).toHaveBeenCalledWith('agent.sessions.activity', { ended_turns: ['trn_1'] });
  expect(sessionTitle(session_id)).toBe('写一个游戏');
  expect(sessionTitle('ses_other')).toBeUndefined();
});
