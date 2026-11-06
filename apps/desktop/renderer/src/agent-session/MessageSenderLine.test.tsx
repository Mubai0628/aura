import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { MessageSenderLine } from './MessageSenderLine';

afterEach(cleanup);

test('a message from a chat names the plugin, the platform and the person who wrote it', () => {
  render(
    <MessageSenderLine
      sender={{
        plugin: 'chat-bot',
        from: {
          channel: 'qq-main',
          platform: 'qq',
          chat: 'group',
          user_id: '10086',
          user_name: '老王',
        },
      }}
    />,
  );
  const line = screen.getByText('来自插件 chat-bot · QQ 群聊 · 老王');
  expect(line.getAttribute('title')).toBe('qq-main · 10086');
});

test("a private chat's sender without a name reads by the platform's id", () => {
  render(
    <MessageSenderLine
      sender={{
        plugin: 'chat-bot',
        from: { channel: 'tg', platform: 'telegram', chat: 'private', user_id: '42' },
      }}
    />,
  );
  expect(screen.getByText('来自插件 chat-bot · Telegram · 42')).toBeTruthy();
});

test("a plugin panel's message names only the plugin; the user's own message shows nothing", () => {
  const { container, rerender } = render(<MessageSenderLine sender={{ plugin: 'usage-panel' }} />);
  expect(screen.getByText('来自插件 usage-panel')).toBeTruthy();
  rerender(<MessageSenderLine sender={null} />);
  expect(container.textContent).toBe('');
});
