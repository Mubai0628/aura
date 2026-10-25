import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, test, vi } from 'vitest';
import { PermissionQuickSelector } from './ComposerQuickMenus';

// The menu stays open while Core answers a mode change; a second pick in that time
// started a second, competing change.
test('one permission change at a time while Core answers', async () => {
  let finish: () => void = () => undefined;
  const onSelect = vi.fn(
    () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
  );
  const { container } = render(
    <PermissionQuickSelector
      demoMode={false}
      mode="workspace_read_write"
      onManageSettings={vi.fn()}
      onSelect={onSelect}
    />,
  );
  fireEvent.click(container.querySelector('.composer-select')!);
  const [first, second] = screen.getAllByRole('menuitemradio') as HTMLButtonElement[];
  fireEvent.click(first!);
  await waitFor(() => expect(second!.disabled).toBe(true));
  fireEvent.click(second!);
  expect(onSelect).toHaveBeenCalledTimes(1);
  finish();
  await waitFor(() => expect(screen.queryByRole('menu')).toBeNull());
});
