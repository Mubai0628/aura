import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { NetworkRequestDetails } from './NetworkRequestDetails';
afterEach(cleanup);

test('the collapsed request shows what is sent and hides only authentication values', () => {
  render(
    <NetworkRequestDetails
      kind="http.request"
      action={{
        url: 'https://api.example.com/items?q=workspace-text',
        method: 'post',
        headers: [
          { name: 'Authorization', value: 'Bearer model-token' },
          { name: 'X-Trace', value: 'visible-trace' },
        ],
        body: { kind: 'json', value: { note: 'sent data' } },
        credential: 'api_key',
        network_request_hash: 'core-only-field',
      }}
    />,
  );
  const details = screen.getByText('查看请求内容').closest('details');
  expect(details?.open).toBe(false);
  const text = details?.textContent ?? '';
  expect(text).toContain('POST');
  expect(text).toContain('https://api.example.com/items?q=workspace-text');
  expect(text).toContain('X-Trace: visible-trace');
  expect(text).toContain('Authorization: （已隐藏）');
  expect(text).not.toContain('model-token');
  expect(text).toContain('"note": "sent data"');
  expect(text).toContain('「api_key」');
  expect(text).not.toContain('core-only-field');
});

test('a long body is previewed with its full length, and a search shows its query', () => {
  const { unmount } = render(
    <NetworkRequestDetails
      kind="http.request"
      action={{
        url: 'http://localhost/a',
        method: 'PUT',
        body: { kind: 'text', text: 'x'.repeat(2500) },
      }}
    />,
  );
  expect(screen.getByText(/正文共 2500 个字符，这里只显示前 2000 个/u)).toBeTruthy();
  expect(screen.getByText('x'.repeat(2000))).toBeTruthy();
  unmount();
  render(<NetworkRequestDetails kind="web.search" action={{ query: 'rust tokio select' }} />);
  expect(screen.getByText('rust tokio select')).toBeTruthy();
  expect(screen.getByText('GET')).toBeTruthy();
  expect(screen.getByText('无正文')).toBeTruthy();
});
