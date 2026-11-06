import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ComputerControlledV1 } from '@aura/contracts/generated';

import { resetSendingForTests, useSending } from '../agent-session/sending';
import { chooseSessionReasoning } from '../reasoning-effort';
import { chooseSessionSpeed } from '../speed-mode';
import { ComputerControlling } from './ComputerControlling';
import { useComputerControlling } from './useComputerControlling';

/**
 * 交还 in 近期活动's 正在操控: the user took the desktop over and gives it back, and the model
 * is told at once, in its running turn or, when the turn has ended, as the next message.
 */
const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const TURN = 'trn_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const MODEL = 'mpr_model';
const NOTEPAD = { app_id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: '记事本' };
let current: ComputerControlledV1[];
const controlling = vi.fn(async () => ({ ok: true as const, value: { apps: current } }));
const stop = vi.fn();
const giveBack = vi.fn();
const submit = vi.fn();
const changed = vi.fn();

function Part({ activeTurn }: { activeTurn?: string }) {
  const state = useComputerControlling(S, { activeTurn, modelId: MODEL, onChanged: changed });
  const sending = useSending(S);
  return (
    <>
      <ComputerControlling controlling={state} />
      <p data-testid="sending">{sending?.text}</p>
    </>
  );
}

beforeEach(() => {
  localStorage.clear();
  chooseSessionReasoning(S, MODEL, null);
  chooseSessionSpeed(S, MODEL, null);
  resetSendingForTests();
  current = [];
  giveBack.mockReset().mockResolvedValue({ ok: true, value: { apps: [] } });
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { computer: { controlling, stop, giveBack }, submitAgentMessage: submit } },
  });
});

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe('交还 after the user took the desktop over', () => {
  it('gives the desktop back to the running turn and tells its model in that turn', async () => {
    chooseSessionReasoning(S, MODEL, 'high');
    chooseSessionSpeed(S, MODEL, 'fast');
    current = [{ ...NOTEPAD, foreground: true, taken_over: true }];
    render(<Part activeTurn={TURN} />);
    await userEvent.click(await screen.findByRole('button', { name: '交还操控 记事本' }));
    expect(giveBack).toHaveBeenCalledWith({ session_id: S });
    await vi.waitFor(() => expect(submit).toHaveBeenCalledTimes(1));
    expect(submit).toHaveBeenCalledWith({
      session_id: S,
      text: '我已交还电脑的操作权，继续。',
      destination: { lane: 'addition', turn_task_id: TURN },
      computer_mode: 'foreground',
      reasoning_effort: 'high',
      speed_mode: 'fast',
    });
    expect(giveBack.mock.invocationCallOrder[0]).toBeLessThan(
      submit.mock.invocationCallOrder[0] ?? 0,
    );
    expect(screen.getByTestId('sending').textContent).toBe('我已交还电脑的操作权，继续。');
    await vi.waitFor(() => expect(screen.queryByText('记事本')).toBeNull());
    expect(changed).toHaveBeenCalled();
  });

  it('after the turn ended, the message starts the next turn on the child desktop', async () => {
    current = [{ ...NOTEPAD, foreground: true, taken_over: true, mode: 'child_session' }];
    render(<Part />);
    expect(await screen.findByText('你已接管小窗')).toBeTruthy();
    await userEvent.click(screen.getByRole('button', { name: '交还操控 记事本' }));
    await vi.waitFor(() => expect(submit).toHaveBeenCalledTimes(1));
    expect(submit).toHaveBeenCalledWith({
      session_id: S,
      text: '我已交还小窗的操作权，继续。',
      destination: { lane: 'next_turn' },
      computer_mode: 'child_session',
    });
  });

  it('sends nothing when the desktop could not be given back, and says so', async () => {
    giveBack.mockResolvedValue({
      ok: false,
      error: { code: 'computer.unavailable', message: 'computer.unavailable: 电脑操控不可用' },
    });
    current = [{ ...NOTEPAD, foreground: true, taken_over: true }];
    render(<Part activeTurn={TURN} />);
    await userEvent.click(await screen.findByRole('button', { name: '交还操控 记事本' }));
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(submit).not.toHaveBeenCalled();
    expect(screen.getByTestId('sending').textContent).toBe('');
    expect(screen.getByText('你已接管')).toBeTruthy();
  });

  it('says when the message was not sent after the desktop was given back', async () => {
    submit.mockResolvedValue({
      ok: false,
      error: { code: 'agent.unavailable', message: 'agent.unavailable: 模型不可用' },
    });
    current = [{ ...NOTEPAD, foreground: true, taken_over: true }];
    render(<Part activeTurn={TURN} />);
    await userEvent.click(await screen.findByRole('button', { name: '交还操控 记事本' }));
    expect(await screen.findByRole('alert')).toBeTruthy();
    expect(giveBack).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('sending').textContent).toBe('');
  });

  it('offers 交还 only on a program the user took over', async () => {
    current = [{ ...NOTEPAD, foreground: true }];
    render(<Part activeTurn={TURN} />);
    expect(await screen.findByRole('button', { name: '停止操控 记事本' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '交还操控 记事本' })).toBeNull();
  });
});
