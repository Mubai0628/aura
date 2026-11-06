import { cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { AgentInboxProjection, AgentPendingMessage } from '@aura/contracts/generated';
import { InboxPanel } from '../agent-session/InboxPanel';
import { MessageText, type MessageTextRole } from '../agent-session/MessageText';
import { SendingMessage } from '../agent-session/SendingMessage';
import { clearMessageTextCache } from '../agent-session/message-text-cache';
import { forgetProgramIconsForTests } from './program-icons';
import { forgetProgramsForTests, knowPrograms } from './program-tokens';

// A program chosen with "@" is sent as `@"name"` in the words; the sent message shows it as the
// composer did, a block with the program's icon.

const NOTEPAD = { app_id: 'Microsoft.WindowsNotepad_8wekyb3d8bbwe!App', name: '记事本' };
/** The first bytes of a PNG: the image is not decoded, only its address is checked. */
const PNG = 'iVBORw0KGgo=';
const catalog = vi.fn();
const icons = vi.fn();
beforeEach(() => {
  clearMessageTextCache();
  forgetProgramsForTests();
  forgetProgramIconsForTests();
  catalog.mockReset().mockResolvedValue({ ok: true, value: { enabled: true, apps: [NOTEPAD] } });
  icons.mockReset().mockResolvedValue({
    ok: true,
    value: { icons: [{ app_id: NOTEPAD.app_id, png: PNG }] },
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { computer: { catalog, icons }, mutateAgentInbox: vi.fn() } },
  });
});
afterEach(cleanup);

/** A message of the conversation, its whole text at hand. */
function shown(text: string, role: MessageTextRole = 'user') {
  return render(
    <MessageText
      sessionId="session"
      source={{ kind: 'user', event_id: 'evt_1' }}
      preview={text}
      totalBytes={new TextEncoder().encode(text).length}
      role={role}
    />,
  );
}

const blocks = (within: ParentNode = document) =>
  [...within.querySelectorAll('.message-program-block')] as HTMLElement[];

test('a sent message shows a program chosen with "@" as a block with its icon', async () => {
  knowPrograms([NOTEPAD]);
  const { container } = shown('用 @"记事本" 打开它');
  await waitFor(() =>
    expect(blocks()[0]?.querySelector('img')?.getAttribute('src')).toBe(
      `data:image/png;base64,${PNG}`,
    ),
  );
  expect(blocks().map((block) => block.textContent)).toEqual(['记事本']);
  expect(container.textContent).toBe('用 记事本 打开它');
});

test('a program not known yet is found among the computer’s programs', async () => {
  shown('用 @"记事本" 打开它');
  await waitFor(() => expect(blocks().map((block) => block.textContent)).toEqual(['记事本']));
  expect(catalog).toHaveBeenCalledTimes(1);
});

test('words that only look like a program, and the model’s answers, stay as written', async () => {
  knowPrograms([NOTEPAD]);
  const { container } = shown('`@"记事本"` 前面@"记事本" 还有 @"画图"');
  await waitFor(() => expect(catalog).toHaveBeenCalled());
  await new Promise((resolve) => setTimeout(resolve, 0));
  expect(blocks()).toEqual([]);
  expect(container.querySelector('code')?.textContent).toBe('@"记事本"');
  expect(container.textContent).toContain('前面@"记事本" 还有 @"画图"');
  cleanup();
  const answer = shown('已经用 @"记事本" 打开了', 'final');
  expect(blocks()).toEqual([]);
  expect(answer.container.textContent).toBe('已经用 @"记事本" 打开了');
});

test('the message being sent and a queued message show the same block', async () => {
  knowPrograms([NOTEPAD]);
  render(<SendingMessage sending={{ text: '用 @"记事本" 打开它', files: [], accepted: false }} />);
  const sending = document.querySelector('.agent-sending') as HTMLElement;
  expect(blocks(sending).map((block) => block.textContent)).toEqual(['记事本']);
  expect(sending.textContent).toContain('用 记事本 打开它');
  cleanup();
  const user = userEvent.setup();
  render(
    <InboxPanel
      inbox={
        {
          session_id: 'ses_1',
          revision: '3',
          paused: false,
          messages: [
            {
              message_id: 'evt_a',
              created_at: '2026-11-03T00:00:00Z',
              destination: { lane: 'next_turn' },
              text: '用 @"记事本" 打开它',
            } as AgentPendingMessage,
          ],
        } as AgentInboxProjection
      }
      activeTurn="trn_1"
      onChanged={vi.fn()}
    />,
  );
  const line = screen.getByRole('button', { name: '用 记事本 打开它' });
  expect(blocks(line).map((block) => block.textContent)).toEqual(['记事本']);
  await user.click(line);
  const full = line.closest('li')?.querySelector('.agent-inbox-full') as HTMLElement;
  await waitFor(() => expect(blocks(full).map((block) => block.textContent)).toEqual(['记事本']));
  expect(within(full).queryByText(/@"/u)).toBeNull();
});
