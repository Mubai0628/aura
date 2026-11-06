import { beforeEach, expect, test, vi } from 'vitest';
import {
  chooseSessionReasoning,
  forgetSessionReasoning,
  sessionReasoning,
} from '../reasoning-effort';
import { chooseSessionSpeed, forgetSessionSpeed, sessionSpeed } from '../speed-mode';
import { branchFrom } from './branch';

// A branch keeps its conversation's own choices for each model: the thinking level and the speed
// mode.

vi.mock('../model-projection-store', () => ({
  readSessionModelSelector: vi.fn(async () => ({
    payload: { selected_model_profile_id: null, binding_revision: '0' },
  })),
}));

const MODEL = 'mdl_01ARZ3NDEKTSV4RRFFQ69G5FAV';
beforeEach(() => {
  localStorage.clear();
  forgetSessionReasoning();
  forgetSessionSpeed();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: { branchSession: vi.fn(async () => ({ ok: true, value: { session_id: 'branch' } })) },
    },
  });
});

test("a branch keeps its conversation's thinking level and speed mode", async () => {
  chooseSessionReasoning('session', MODEL, 'high');
  chooseSessionSpeed('session', MODEL, 'fast');
  await branchFrom('session', 'turn');
  expect(sessionReasoning('branch', MODEL)).toBe('high');
  expect(sessionSpeed('branch', MODEL)).toBe('fast');
});
