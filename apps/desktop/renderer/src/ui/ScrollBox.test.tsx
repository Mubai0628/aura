import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ScrollBox } from './ScrollBox';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

/** jsdom lays nothing out: the box's content and visible heights are given. */
function heights(content: number, visible: number) {
  vi.spyOn(Element.prototype, 'scrollHeight', 'get').mockReturnValue(content);
  vi.spyOn(Element.prototype, 'clientHeight', 'get').mockReturnValue(visible);
}

// A step's expanded content that does not fit is read in a dashed box that
// scrolls by itself, instead of pushing the whole conversation down.
test('content longer than the box shows the dashed frame and scrolls from the keyboard too', () => {
  heights(900, 360);
  render(
    <ScrollBox label="命令详情">
      <p>很长的命令</p>
    </ScrollBox>,
  );
  const box = screen.getByRole('region', { name: '命令详情' });
  expect(box.dataset.overflowing).toBe('true');
  expect(box.tabIndex).toBe(0);
});

test('content that fits keeps no frame and no extra tab stop', () => {
  heights(120, 120);
  const { container } = render(
    <ScrollBox label="命令详情">
      <p>短命令</p>
    </ScrollBox>,
  );
  const box = container.querySelector('.scroll-box') as HTMLElement;
  expect(box.dataset.overflowing).toBeUndefined();
  expect(box.hasAttribute('tabindex')).toBe(false);
  expect(screen.queryByRole('region')).toBeNull();
});
