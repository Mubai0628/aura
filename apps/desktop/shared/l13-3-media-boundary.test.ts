import { expect, it, vi } from 'vitest';
import type { IpcMainInvokeEvent } from 'electron';
import { dispatchDomainRoute } from '../electron-main/domain-route.js';
import { isAgentCommand, validateAgentRequest } from './agent-validation.js';

it('keeps media byte and result authority on the authenticated Core channel', async () => {
  const agent = vi.fn();
  const handlers = {
    agent,
    workspace: vi.fn(),
    security: vi.fn(),
    model: vi.fn(),
    tool: vi.fn(),
    git: vi.fn(),
    package: vi.fn(),
    memory: vi.fn(),
  };
  for (const command of ['media.host.read', 'media.host.result', 'media.probe', 'media.frames']) {
    const value = {
      command_id: 'mcm-forged',
      runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      offset: 0,
      length: 1,
      path: 'C:/private/another-file.mp4',
      actor: 'authenticated_main',
    };
    expect(isAgentCommand(command)).toBe(false);
    expect(validateAgentRequest({ command, value })).toBe(false);
    await expect(
      dispatchDomainRoute({} as IpcMainInvokeEvent, command, value, handlers),
    ).rejects.toThrow();
  }
  expect(agent).not.toHaveBeenCalled();
});
