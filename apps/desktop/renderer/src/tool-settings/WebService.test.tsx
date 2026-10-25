import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';

import { WebService } from './WebService';
import type { ConfigurationOwner } from './useAgentConfiguration';

afterEach(cleanup);

function owner(view: Partial<NonNullable<ConfigurationOwner['view']>>): ConfigurationOwner {
  return {
    view: { revision: '3', ...view } as NonNullable<ConfigurationOwner['view']>,
    loadError: undefined,
    busy: false,
    reload: vi.fn(),
    commit: vi.fn().mockResolvedValue({}),
  };
}

// DeepSeek's web search and fetch through a DeepSeek connection's key,
// automatic by default, or this computer's own.
test('the DeepSeek connections are offered and a choice is saved', async () => {
  const configuration = owner({
    web_service: { kind: 'automatic' },
    web_service_connections: [{ endpoint_id: 'end_A', display_name: 'DeepSeek Responses' }],
  });
  render(<WebService configuration={configuration} />);
  const select = screen.getByRole('combobox');
  expect(screen.getByRole('option', { name: 'DeepSeek · DeepSeek Responses' })).toBeTruthy();
  expect((select as HTMLSelectElement).value).toBe('automatic');
  fireEvent.change(select, { target: { value: 'deepseek:end_A' } });
  await waitFor(() =>
    expect(configuration.commit).toHaveBeenCalledWith({
      web_service: { kind: 'deepseek', endpoint_id: 'end_A' },
    }),
  );
  fireEvent.change(select, { target: { value: 'local' } });
  await waitFor(() =>
    expect(configuration.commit).toHaveBeenLastCalledWith({ web_service: { kind: 'local' } }),
  );
});

test('a chosen connection that is gone still shows, so the choice is visible', () => {
  render(
    <WebService
      configuration={owner({
        web_service: { kind: 'deepseek', endpoint_id: 'end_gone' },
        web_service_connections: [],
      })}
    />,
  );
  expect((screen.getByRole('combobox') as HTMLSelectElement).value).toBe('deepseek:end_gone');
  expect(screen.getByRole('option', { name: 'DeepSeek · 已不可用的连接' })).toBeTruthy();
});
