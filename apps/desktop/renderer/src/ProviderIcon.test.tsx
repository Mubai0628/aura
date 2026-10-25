import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, test } from 'vitest';
import { ProviderIcon } from './ProviderIcon';
import { providerInitials } from './model-settings-types';
afterEach(cleanup);
test('uses first two uppercase letters without a logo and after image failure', () => {
  const initials = providerInitials('abliteration.ai');
  expect(initials).toBe('AB');
  const view = render(<ProviderIcon fallbackInitial={initials} iconUrl={undefined} />);
  expect(screen.getByText('AB')).toBeTruthy();
  view.rerender(<ProviderIcon fallbackInitial={initials} iconUrl="aura://missing-icon" />);
  const image = view.container.querySelector('img');
  expect(image).not.toBeNull();
  fireEvent.error(image!);
  expect(screen.getByText('AB')).toBeTruthy();
});
