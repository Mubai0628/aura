import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test } from 'vitest';
import { HelpTip } from './HelpTip';

// A modal dialog is drawn above everything else on the page, so the bubble of a question mark
// inside one is drawn in that dialog; elsewhere it is drawn over the page.

afterEach(cleanup);

const hint = () => screen.getByRole('img', { name: '快速模式' });

test('the bubble of a question mark in a dialog is drawn in that dialog', async () => {
  const user = userEvent.setup();
  render(
    <dialog open>
      <HelpTip label="快速模式" text="请确保你的提供商支持该模式" />
    </dialog>,
  );
  await user.hover(hint());
  expect(screen.getByRole('tooltip').closest('dialog')).not.toBeNull();
  await user.unhover(hint());
  expect(screen.queryByRole('tooltip')).toBeNull();
});

test('elsewhere the bubble is drawn over the page', async () => {
  const user = userEvent.setup();
  render(<HelpTip label="快速模式" text="请确保你的提供商支持该模式" />);
  await user.hover(hint());
  expect(screen.getByRole('tooltip').parentElement).toBe(document.body);
});
