import { configure } from '@testing-library/react';
import { afterEach } from 'vitest';
import { forgetTurnChoices } from './agent-session/turn-choices';
import { forgetToolCallDirectories } from './agent-session/useToolCallDirectory';

/**
 * Waiting for an element is a wait for a promise, and the suite runs many test
 * files across two workers: the default one-second budget is short enough that a
 * loaded machine fails a test that is not actually broken (observed as
 * `findByRole` timeouts just over 1000 ms in `tool-settings/AgentToolList`).
 *
 * Raising the budget does not hide a missing element: a query that can never
 * succeed still fails, it just fails after a longer wait.
 */
configure({ asyncUtilTimeout: 5_000 });

/**
 * Chromium has `ResizeObserver`; jsdom does not. A component that measures itself (the Radix
 * switch inside a form does) would fail to render in a test, so a test without its own stub gets
 * one that observes nothing. Tests that need real callbacks install their own.
 */
if (typeof globalThis.ResizeObserver === 'undefined') {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

/**
 * jsdom lays nothing out and has no `scrollIntoView`: a component that brings an element into view
 * after a click (the review page's file list, a thought's title) would throw in a later frame.
 * Tests that check the call install their own and put this one back.
 */
if (typeof Element !== 'undefined' && typeof Element.prototype.scrollIntoView !== 'function') {
  Element.prototype.scrollIntoView = () => {};
}

/** jsdom has the dialog element, but does not enter Chromium's modal top layer. */
if (
  typeof HTMLDialogElement !== 'undefined' &&
  typeof HTMLDialogElement.prototype.showModal !== 'function'
) {
  HTMLDialogElement.prototype.showModal = function () {
    this.open = true;
  };
}

/**
 * jsdom has no canvas: its `getContext` logs "not implemented" and gives nothing. The context
 * card's fluid then shows its plain bands; this answers the same, without the noise.
 */
if (typeof HTMLCanvasElement !== 'undefined')
  HTMLCanvasElement.prototype.getContext = (() =>
    null) as typeof HTMLCanvasElement.prototype.getContext;

/** What the window keeps of each conversation (its calls, the user's choices in each turn)
 * starts empty in every test. */
afterEach(() => {
  forgetToolCallDirectories();
  forgetTurnChoices();
});
