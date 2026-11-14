import { cleanup, render, screen } from '@testing-library/react';
import { useRef } from 'react';
import { afterEach, expect, test, vi } from 'vitest';
import { useComposerDockClearance } from './useComposerDockClearance';

// The room the conversation leaves under its last message is the composer's own height. A menu
// opened from the composer (+, 权限, 模型, 思考程度) floats over the conversation; it does not make
// that room taller, so a conversation scrolled to the bottom is not pushed up.

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const rect = (top: number, height: number) =>
  ({
    top,
    bottom: top + height,
    height,
    left: 0,
    right: 600,
    width: 600,
    x: 0,
    y: top,
    toJSON: () => ({}),
  }) as DOMRect;

function Probe({ menu }: { menu?: string }) {
  const dockRef = useRef<HTMLDivElement>(null);
  const clearance = useComposerDockClearance(dockRef);
  return (
    <>
      <output aria-label="clearance">{clearance ?? ''}</output>
      <div className="composer-dock" ref={dockRef}>
        {menu ? <div className={menu} /> : null}
      </div>
    </>
  );
}

test('an open composer menu leaves the room under the conversation as it was', () => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains('composer-dock')) return rect(600, 120);
    if (this.classList.contains('composer-popover') || this.classList.contains('attachment-menu'))
      return rect(300, 290);
    return rect(0, 0);
  });
  const clearance = () => screen.getByRole('status', { name: 'clearance' }).textContent;
  render(<Probe />);
  expect(clearance()).toBe('120');
  cleanup();
  for (const menu of ['composer-popover', 'attachment-menu']) {
    render(<Probe menu={menu} />);
    expect(clearance(), menu).toBe('120');
    cleanup();
  }
});
