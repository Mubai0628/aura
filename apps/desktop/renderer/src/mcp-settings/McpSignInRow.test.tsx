import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { McpServerView } from '@aura/contracts/generated';
import { McpSignInRow } from './McpSignInRow';

// The sign-in address the browser did not open is shown on its own line, broken anywhere, with a
// button that copies it through Main's clipboard.

const LINK =
  'https://github.com/login/oauth/authorize?response_type=code&client_id=aura&redirect_uri=http%3A%2F%2F127.0.0.1%3A53682%2Fcallback&code_challenge=Wm9vbV9jaGFsbGVuZ2VfdmFsdWVfdGhhdF9pc19sb25n&code_challenge_method=S256&state=c3RhdGVfdmFsdWVfdGhhdF9pc19sb25nX2Fz';

const server: McpServerView = {
  config: {
    id: 'github01',
    display_name: 'Github',
    transport: { type: 'http', url: 'https://api.githubcopilot.com/mcp/', headers: [] },
    enabled: true,
    trusted_fingerprint: 'github01-fingerprint',
    startup_timeout_ms: 30_000,
    call_timeout_ms: 120_000,
    available_for_subagents: false,
  },
  state: 'sign_in_required',
  fingerprint: 'github01-fingerprint',
  problems: [],
  running_instances: 0,
  sign_in: { signed_in: false, pending: true, authorization_server: 'github.com' },
};

const writeClipboard = vi.fn();
beforeEach(() => {
  writeClipboard.mockReset();
  Object.defineProperty(window, 'aura', { configurable: true, value: { v1: { writeClipboard } } });
});
afterEach(cleanup);

const row = () =>
  render(
    <McpSignInRow
      server={server}
      link={LINK}
      locked={false}
      onSignIn={vi.fn()}
      onSignOut={vi.fn()}
    />,
  );

test('the address sits in its own block that breaks anywhere, with a copy button', () => {
  const { container } = row();
  const address = container.querySelector('code.mcp-sign-in-link');
  expect(address?.textContent).toBe(LINK);
  expect(screen.getByRole('button', { name: '复制链接' })).toBeTruthy();
});

test('copying goes through Main and says so', async () => {
  const user = userEvent.setup();
  writeClipboard.mockResolvedValue({ ok: true, value: { completed: true, message: '已复制' } });
  row();
  await user.click(screen.getByRole('button', { name: '复制链接' }));
  expect(writeClipboard).toHaveBeenCalledWith({ text: LINK });
  expect((await screen.findByRole('status')).textContent).toBe('已复制');
});

test('a copy that failed says how to copy by hand', async () => {
  const user = userEvent.setup();
  writeClipboard.mockResolvedValue({
    ok: false,
    error: { code: 'clipboard.failed', message: 'x' },
  });
  row();
  await user.click(screen.getByRole('button', { name: '复制链接' }));
  expect((await screen.findByRole('alert')).textContent).toBe(
    '链接没有复制成功，请选中地址后手动复制。',
  );
});
