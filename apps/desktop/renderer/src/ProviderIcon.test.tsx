import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
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

test('a provider picture loads only when it is about to be seen', () => {
  const view = render(
    <ProviderIcon fallbackInitial="AB" iconUrl="aura://app/provider-icon/art/hash.png" />,
  );
  const image = view.container.querySelector('img');
  expect(image?.getAttribute('loading')).toBe('lazy');
  expect(image?.getAttribute('decoding')).toBe('async');
});

test('a provider picture is not asked for until it comes near the view', () => {
  const watchers: IntersectionObserverCallback[] = [];
  const original = window.IntersectionObserver;
  window.IntersectionObserver = class {
    constructor(callback: IntersectionObserverCallback) {
      watchers.push(callback);
    }
    observe() {}
    disconnect() {}
  } as unknown as typeof IntersectionObserver;
  try {
    const view = render(
      <ProviderIcon fallbackInitial="AB" iconUrl="aura://app/provider-icon/art/hash.png" />,
    );
    expect(view.container.querySelector('img')).toBeNull();
    act(() =>
      watchers.at(-1)!(
        [{ isIntersecting: true } as IntersectionObserverEntry],
        {} as IntersectionObserver,
      ),
    );
    expect(view.container.querySelector('img')?.getAttribute('src')).toBe(
      'aura://app/provider-icon/art/hash.png',
    );
  } finally {
    window.IntersectionObserver = original;
  }
});
