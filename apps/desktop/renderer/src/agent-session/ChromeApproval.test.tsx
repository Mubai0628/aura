import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ApprovalPanel } from './ApprovalPanel';

afterEach(cleanup);

function install(
  action: string,
  capabilities: string[],
  origin: { origin: string; confirmed: boolean },
  decisions: string[],
) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            mode: 'read_only',
            backend: { id: 'agent-chrome-v1', class: 'core_broker', guarantees: [] },
            operation: {
              content: {
                action_kind: action,
                action: {},
                purpose: '看购物车',
                resources: [
                  { scope_kind: 'chrome_share', share_id: 'share-1' },
                  { scope_kind: 'chrome_tab', tab_ref: 't1' },
                  { scope_kind: 'chrome_origin', ...origin },
                ],
                capabilities,
                credentials: [],
              },
            },
            requested_capabilities: capabilities,
            reasons: ['policy.approval_required'],
            approval: {
              revision: 1,
              expires_at: '2026-10-27T00:00:00Z',
              state: 'pending',
              allowed_decisions: decisions,
            },
          },
        }),
        resolveAgentApproval: async () => ({ ok: true, value: {} }),
      },
    },
  });
}

const choices = () =>
  screen
    .getAllByRole('button')
    .map((button) => button.textContent)
    .filter((text) => ['仅此一次', '本轮允许', '本会话允许此站点', '拒绝'].includes(text ?? ''));

test('acting in a shared tab in ReadOnly names the tab and site, and may cover the turn', async () => {
  install(
    'chrome.click',
    ['chrome_interaction'],
    { origin: 'https://shop.example', confirmed: true },
    ['allow_once', 'allow_for_current_turn', 'deny'],
  );
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  const facts = await screen.findByRole('region', { name: 'Chrome 操作' });
  expect(screen.getByText('在 Chrome 中点击')).toBeTruthy();
  expect(facts.textContent).toContain('t1');
  expect(facts.textContent).toContain('https://shop.example');
  expect(facts.textContent).toContain('敏感操作仍会每次询问');
  expect(screen.queryByRole('alert')).toBeNull();
  expect(choices()).toEqual(['仅此一次', '本轮允许', '拒绝']);
});

test('a sensitive action says what it may do and that only once can be allowed', async () => {
  install(
    'chrome.click',
    ['chrome_sensitive_action'],
    { origin: 'https://shop.example', confirmed: true },
    ['allow_once', 'deny'],
  );
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  expect((await screen.findByRole('alert')).textContent).toContain('购买、付款、删除');
  expect(choices()).toEqual(['仅此一次', '拒绝']);
});

test('a site not confirmed yet is named first and may be allowed for the session', async () => {
  install('chrome.snapshot', ['chrome_read'], { origin: 'https://pay.example', confirmed: false }, [
    'allow_once',
    'allow_session',
    'deny',
  ]);
  render(<ApprovalPanel sessionId="ses" approvalId="apr" onChanged={() => undefined} />);
  const alert = await screen.findByRole('alert');
  expect(alert.textContent).toContain('https://pay.example 是本会话还没有确认过的站点');
  expect(screen.getByText('（尚未确认）')).toBeTruthy();
  expect(choices()).toEqual(['仅此一次', '本会话允许此站点', '拒绝']);
  expect(screen.queryByText(/「本会话允许」只覆盖这一类操作/u)).toBeNull();
});
