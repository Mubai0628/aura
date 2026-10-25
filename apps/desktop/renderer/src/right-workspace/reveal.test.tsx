import { act, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, test } from 'vitest';
import { App } from '../App';
import { installAppTestEnvironment } from '../app-test-fixtures';
import { toolTabs } from './tool-tabs';

installAppTestEnvironment();

// "查看终端" on the call that opened the model's terminal shows the right
// sidebar with that terminal's tab, even when the sidebar was closed.
test("a link to the model's terminal opens the closed right sidebar on its tab", async () => {
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole('button', { name: '关闭右侧边栏' }));
  expect(screen.queryByRole('complementary', { name: '右侧工作区' })).toBeNull();
  act(() => toolTabs.revealTerminal('ses_01ARZ3NDEKTSV4RRFFQ69G5FAV', 'trm_model'));
  expect(screen.getByRole('complementary', { name: '右侧工作区' })).toBeTruthy();
  // The fixture lists no such terminal, so its tab keeps the plain name.
  expect(screen.getByRole('tab', { name: '终端' }).getAttribute('aria-selected')).toBe('true');
});
