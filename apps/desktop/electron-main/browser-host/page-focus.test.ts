// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { PRESSED_MS, PagePresses, SETTLE_MS, keepKeyboardForUser } from './page-focus.js';

//

class Contents {
  focused = false;
  destroyed = false;
  #focus: (() => void)[] = [];
  on(_event: 'focus', listener: () => void) {
    this.#focus.push(listener);
    return this;
  }
  isFocused() {
    return this.focused;
  }
  isDestroyed() {
    return this.destroyed;
  }
  focus() {
    this.focused = true;
    for (const listener of this.#focus) listener();
  }
}

function watched() {
  const contents = new Contents();
  const presses = new PagePresses();
  const giveBack = vi.fn(() => {
    contents.focused = false;
  });
  const later: { run: () => void; ms: number }[] = [];
  keepKeyboardForUser(contents, presses, giveBack, (run, ms) => later.push({ run, ms }));
  const settle = () => {
    for (const { run } of later.splice(0)) run();
  };
  return { contents, presses, giveBack, later, settle };
}

test('a page that takes the focus on its own gives it back to Aura', () => {
  const { contents, giveBack, later, settle } = watched();
  contents.focus();
  expect(later.map(({ ms }) => ms)).toEqual([SETTLE_MS]);
  settle();
  expect(giveBack).toHaveBeenCalledTimes(1);
  expect(contents.focused).toBe(false);
});

test("the user's own press on the page keeps its focus", () => {
  const { contents, presses, giveBack, settle } = watched();
  contents.focus();
  // The press arrives just after the focus it caused.
  presses.pressed();
  settle();
  expect(giveBack).not.toHaveBeenCalled();
  expect(contents.focused).toBe(true);
});

test('an old press does not hold the focus for a later load', () => {
  const { contents, presses, giveBack, settle } = watched();
  presses.pressed(Date.now() - PRESSED_MS - 1);
  contents.focus();
  settle();
  expect(giveBack).toHaveBeenCalledTimes(1);
});

test('a focus already gone, or a closed page, is left alone', () => {
  const { contents, giveBack, settle } = watched();
  contents.focus();
  contents.focused = false;
  settle();
  contents.focus();
  contents.destroyed = true;
  settle();
  expect(giveBack).not.toHaveBeenCalled();
});
