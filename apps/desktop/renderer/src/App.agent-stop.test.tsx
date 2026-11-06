import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment } from './app-test-fixtures';

installAppTestEnvironment();

const RUN = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW';
const TURN = 'trn_01ARZ3NDEKTSV4RRFFQ69G5FAX';

/** A conversation whose one turn is running. */
async function runningTurn() {
  const history = vi.mocked(window.aura.v1.readAgentHistory);
  const page = (await history({} as never)) as unknown as { ok: true; value: object };
  history.mockResolvedValue({
    ok: true,
    value: {
      ...page.value,
      active: {
        run: { agent_run_id: RUN, revision: '1', state: { state: 'running' } },
        turn: { turn_task_id: TURN },
        pending_approval_ids: [],
      },
      turns: [
        {
          turn: {
            session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
            turn_task_id: TURN,
            agent_run_id: RUN,
            mode: 'normal',
            revision: '1',
            state: { state: 'running' },
          },
          user_messages: [],
          assistant_messages: [],
        },
      ],
    },
  } as never);
}

/** Core's answer to the 停止 of this test, given when the test says. */
function heldStop() {
  let answer: (result: unknown) => void = () => undefined;
  const cancel = vi.fn(() => new Promise((resolve) => (answer = resolve)));
  const stopCommand = {
    name: 'stop',
    title: '停止当前任务',
    description: '停止当前任务',
    kind: 'immediate',
    source: 'builtin',
    argument_hint: null,
    template: null,
    enabled: true,
    issue: null,
    location: null,
    revision: null,
  };
  Object.assign(window.aura.v1, {
    cancelAgentRun: cancel,
    listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [stopCommand] } }),
  });
  return { cancel, answer: (result: unknown) => act(async () => answer(result)) };
}

// Core answers a 停止 once the run has really stopped, which took seconds; the page showed
// nothing meanwhile, and the button and /stop took the same stop again.
test('停止 says it is stopping from the click on and is not sent again before Core answers', async () => {
  await runningTurn();
  const stop = heldStop();
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('button', { name: '停止当前任务' }));
  const waiting = screen.getByRole<HTMLButtonElement>('button', { name: '正在停止' });
  expect(waiting.disabled).toBe(true);
  expect(document.querySelector('.agent-turn-status')?.textContent).toContain('正在停止');
  fireEvent.click(waiting);
  await user.type(screen.getByLabelText('输入消息'), '/sto{Enter}');
  expect(stop.cancel).toHaveBeenCalledTimes(1);
  await stop.answer({ ok: true, value: {} });
  expect(stop.cancel).toHaveBeenCalledTimes(1);
});

// A stop Core refused leaves the run as it was; one whose last step is still ending stays a
// stop, and the notice asks for another click, so the button takes one again either way.
test.each([
  {
    case: 'refused',
    error: { code: 'agent.cancel_unconfirmed', message: 'agent.cancel_unconfirmed: 停止未确认。' },
    turnSays: '正在思考',
  },
  {
    case: 'still ending',
    error: { code: 'agent.stop_pending', message: 'agent.stop_pending: run is still alive' },
    turnSays: '正在停止',
  },
])('after a stop Core did not finish, 停止 can be asked again ($case)', async (example) => {
  await runningTurn();
  const stop = heldStop();
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('button', { name: '停止当前任务' }));
  await stop.answer({ ok: false, error: example.error });
  const again = await screen.findByRole<HTMLButtonElement>('button', { name: '停止当前任务' });
  expect(again.disabled).toBe(false);
  expect(document.querySelector('.agent-turn-status')?.textContent).toContain(example.turnSays);
  await user.click(again);
  expect(stop.cancel).toHaveBeenCalledTimes(2);
});

test('a stop whose reply never came back takes back 正在停止', async () => {
  await runningTurn();
  Object.assign(window.aura.v1, { cancelAgentRun: vi.fn().mockRejectedValue(new Error('gone')) });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('button', { name: '停止当前任务' }));
  expect(await screen.findByRole('button', { name: '停止当前任务' })).toBeTruthy();
  expect(document.querySelector('.agent-turn-status')?.textContent).not.toContain('正在停止');
});

// The session view kept a notification list of its own that nothing displayed, so a stop
// Core did not confirm was never shown; it now goes to the one list the app shows.
test('a stop that Core does not confirm is shown to the user', async () => {
  const history = vi.mocked(window.aura.v1.readAgentHistory);
  const page = (await history({} as never)) as unknown as { ok: true; value: object };
  history.mockResolvedValue({
    ok: true,
    value: {
      ...page.value,
      active: {
        run: {
          agent_run_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW',
          revision: '1',
          state: { state: 'running' },
        },
        turn: { turn_task_id: 'trn_01ARZ3NDEKTSV4RRFFQ69G5FAX' },
        pending_approval_ids: [],
      },
    },
  } as never);
  const cancel = vi.fn().mockResolvedValue({
    ok: false,
    error: {
      code: 'agent.cancel_unconfirmed',
      message: 'agent.cancel_unconfirmed: 停止的结果还没有确认，请查看执行状态。',
    },
  });
  Object.assign(window.aura.v1, { cancelAgentRun: cancel });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('button', { name: '停止当前任务' }));
  const notices = await screen.findByRole('region', { name: '系统消息' }).catch(() => undefined);
  const viewport = notices ?? document.querySelector('.notification-viewport');
  expect(cancel).toHaveBeenCalledTimes(1);
  const alert = await within(viewport as HTMLElement).findByRole('alert');
  expect(alert.textContent).toContain('停止未确认');
  expect(alert.textContent).toContain('停止的结果还没有确认，请查看执行状态。');
});

// 停止 showed only "停止未确认" for a run Core was still stopping and for a stop
// whose record could not be written. Each now says why, what it means and what to do next.
test.each([
  {
    case: 'still stopping',
    error: {
      code: 'agent.stop_pending',
      message: 'agent.stop_pending: run run_01 is still alive in stage tools after 45012 ms',
    },
    says: ['还没有完全停下', '再点一次'],
  },
  {
    case: 'no reply in time',
    error: { code: 'request_timeout', message: 'Core request timed out: agent.cancel' },
    says: ['没有及时回复', '再点一次', '重启 Aura'],
  },
  {
    case: 'its record could not be written',
    error: {
      code: 'persistence.conflict',
      message: 'the run could not be interrupted: persistent revision or identity conflict',
    },
    says: ['没有确认', '再点一次', '重启 Aura'],
  },
])('a stop that did not finish says why and what to do ($case)', async ({ error, says }) => {
  const history = vi.mocked(window.aura.v1.readAgentHistory);
  const page = (await history({} as never)) as unknown as { ok: true; value: object };
  history.mockResolvedValue({
    ok: true,
    value: {
      ...page.value,
      active: {
        run: {
          agent_run_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAW',
          revision: '1',
          state: { state: 'tools_running' },
        },
        turn: { turn_task_id: 'trn_01ARZ3NDEKTSV4RRFFQ69G5FAX' },
        pending_approval_ids: [],
      },
    },
  } as never);
  Object.assign(window.aura.v1, {
    cancelAgentRun: vi.fn().mockResolvedValue({ ok: false, error }),
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('button', { name: '停止当前任务' }));
  // A notice that is not a failure leaves by itself after 4 s: look for it at once.
  const viewport = document.querySelector('.notification-viewport');
  // A stop still ending is news, not an alarm.
  const alert = await within(viewport as HTMLElement).findByRole(
    error.code === 'agent.stop_pending' ? 'status' : 'alert',
  );
  for (const words of says) expect(alert.textContent).toContain(words);
  expect(alert.textContent).not.toMatch(/[a-z]+\.[a-z_]+|run_01|ms\b|Core/u);
});
