import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment } from './app-test-fixtures';

installAppTestEnvironment();

test('the model page holds the network proxy for model requests and tools', async () => {
  const user = userEvent.setup();
  render(<App />);
  await user.click(screen.getByRole('button', { name: '设置' }));
  expect(screen.getByRole('heading', { level: 2, name: '模型配置' })).toBeTruthy();
  const proxy = await screen.findByRole('region', { name: '网络代理' });
  expect(within(proxy).getByRole('heading', { level: 3, name: '网络代理' })).toBeTruthy();
  expect(await within(proxy).findByRole('switch', { name: '启用网络代理' })).toBeTruthy();
  expect(within(proxy).queryByRole('switch', { name: '跟随系统代理' })).toBeNull();
});
