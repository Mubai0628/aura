import { render, screen } from '@testing-library/react';
import { expect, test, vi } from 'vitest';

import { ToggleSwitch } from './ToggleSwitch';

// A settings page opens before Core has answered what its switches are. A switch shown as off and
// then on once the answer arrives reads as the setting changing by itself, so until the value is
// known the switch claims neither and cannot be pressed.

test('a switch whose value is still being read is neither on nor off', () => {
  const view = render(
    <ToggleSwitch checked={undefined} label="启用内置浏览器" onCheckedChange={vi.fn()} />,
  );
  const control = screen.getByRole('switch', { name: '启用内置浏览器' }) as HTMLButtonElement;
  expect(control.dataset.state).toBeUndefined();
  expect(control.getAttribute('aria-busy')).toBe('true');
  expect(control.disabled).toBe(true);
  expect(control.querySelector('.aura-switch-thumb')?.getAttribute('data-state')).toBeNull();

  view.rerender(<ToggleSwitch checked label="启用内置浏览器" onCheckedChange={vi.fn()} />);
  expect(control.dataset.state).toBe('checked');
  expect(control.getAttribute('aria-checked')).toBe('true');
  expect(control.hasAttribute('aria-busy')).toBe(false);
  expect(control.disabled).toBe(false);
});

test('a known value is shown as it is', () => {
  render(<ToggleSwitch checked={false} label="允许模型调试网页" onCheckedChange={vi.fn()} />);
  const control = screen.getByRole('switch', { name: '允许模型调试网页' });
  expect(control.dataset.state).toBe('unchecked');
  expect(control.getAttribute('aria-checked')).toBe('false');
});
