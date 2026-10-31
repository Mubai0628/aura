import { act, cleanup, render } from '@testing-library/react';
import { useRef } from 'react';
import { afterEach, expect, test, vi } from 'vitest';
import { useConversationScroll } from './useConversationScroll';

// Every switch to another conversation opens it at its last message. The
// position used to be restored per Core runtime, which every conversation shares, so a switch kept
// the old position; at the top the "earlier messages" sentinel then loaded page after page.

afterEach(cleanup);

function Harness({ session, version }: { session: string; version: string }) {
  const ref = useRef<HTMLDivElement>(null);
  useConversationScroll(ref, session, 'run_1', version);
  return <div ref={ref} data-testid="viewport" />;
}

/** A viewport 400 px high over `height` px of content, clamped like a real one. */
function measure(element: HTMLElement, height: number) {
  let top = 0;
  Object.defineProperty(element, 'scrollHeight', { configurable: true, get: () => height });
  Object.defineProperty(element, 'clientHeight', { configurable: true, get: () => 400 });
  Object.defineProperty(element, 'scrollTop', {
    configurable: true,
    get: () => top,
    set: (value: number) => {
      top = Math.max(0, Math.min(value, height - 400));
    },
  });
}

function scrollTo(element: HTMLElement, top: number) {
  element.scrollTop = top;
  element.dispatchEvent(new Event('scroll'));
}

test('switching to another conversation opens it at its last message', () => {
  const { getByTestId, rerender } = render(<Harness session="ses_a" version="a1" />);
  const viewport = getByTestId('viewport');
  measure(viewport, 5000);
  rerender(<Harness session="ses_a" version="a2" />);
  expect(viewport.scrollTop).toBe(4600);
  // The reader scrolls up in the first conversation, then switches.
  scrollTo(viewport, 100);
  rerender(<Harness session="ses_b" version="b1" />);
  expect(viewport.scrollTop).toBe(4600);
  // Back again: at the last message, not where the reader left it.
  scrollTo(viewport, 0);
  rerender(<Harness session="ses_a" version="a2" />);
  expect(viewport.scrollTop).toBe(4600);
});

/** The wheel turned up, and the first frames of its smooth scroll: a few pixels from the end. */
function wheelUp(element: HTMLElement, top: number) {
  element.dispatchEvent(new WheelEvent('wheel', { deltaY: -100 }));
  scrollTo(element, top);
}

// Scrolling up from the end counted as following until the reader was 90 px up, so new output
// pulled them back to the end while the wheel was still turning: the view flashed and stuck.
test('a wheel turned up at the end stops following at once, and the end is followed again', () => {
  const { getByTestId, rerender } = render(<Harness session="ses_a" version="a1" />);
  const viewport = getByTestId('viewport');
  measure(viewport, 5000);
  rerender(<Harness session="ses_a" version="a2" />);
  expect(viewport.scrollTop).toBe(4600);
  act(() => wheelUp(viewport, 4570));
  rerender(<Harness session="ses_a" version="a3" />);
  expect(viewport.scrollTop).toBe(4570);
  // Back down at the end, new output is followed again.
  act(() => scrollTo(viewport, 4600));
  measure(viewport, 5100);
  rerender(<Harness session="ses_a" version="a4" />);
  expect(viewport.scrollTop).toBe(4700);
});

test('content growing while the reader scrolls up does not pull them back', () => {
  const grown: (() => void)[] = [];
  class Observer {
    constructor(callback: () => void) {
      grown.push(callback);
    }
    observe(): void {
      /* the test says when content grew */
    }
    disconnect(): void {
      /* nothing to release */
    }
  }
  vi.stubGlobal('ResizeObserver', Observer);
  try {
    const { getByTestId, rerender } = render(<Harness session="ses_a" version="a1" />);
    const viewport = getByTestId('viewport');
    measure(viewport, 5000);
    rerender(<Harness session="ses_a" version="a2" />);
    act(() => wheelUp(viewport, 4570));
    act(() => grown.at(-1)?.());
    expect(viewport.scrollTop).toBe(4570);
  } finally {
    vi.unstubAllGlobals();
  }
});

test('within a conversation, reading older messages is not interrupted by new output', () => {
  const { getByTestId, rerender } = render(<Harness session="ses_a" version="a1" />);
  const viewport = getByTestId('viewport');
  measure(viewport, 5000);
  rerender(<Harness session="ses_a" version="a2" />);
  scrollTo(viewport, 1000);
  rerender(<Harness session="ses_a" version="a3" />);
  expect(viewport.scrollTop).toBe(1000);
});

function Reader({ version }: { version: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const { away, hasNew, follow } = useConversationScroll(ref, 'ses_a', 'run_1', version);
  return (
    <div ref={ref} data-testid="viewport" data-away={String(away)} data-new={String(hasNew)}>
      <button type="button" onClick={follow}>
        follow
      </button>
    </div>
  );
}

// The way back to the latest message is offered whenever some of the conversation is below the
// window, new content or not; new content while away adds its dot.
test('reading above the end offers the way back, and new content marks it', () => {
  const { getByTestId, getByText, rerender } = render(<Reader version="a1" />);
  const viewport = getByTestId('viewport');
  measure(viewport, 5000);
  rerender(<Reader version="a2" />);
  expect(viewport.dataset.away).toBe('false');
  // A few pixels short of the end is still the end.
  act(() => scrollTo(viewport, 4590));
  expect(viewport.dataset.away).toBe('false');
  act(() => scrollTo(viewport, 3000));
  expect(viewport.dataset.away).toBe('true');
  expect(viewport.dataset.new).toBe('false');
  rerender(<Reader version="a3" />);
  expect(viewport.dataset.new).toBe('true');
  act(() => getByText('follow').click());
  expect(viewport.scrollTop).toBe(4600);
  expect(viewport.dataset.away).toBe('false');
  expect(viewport.dataset.new).toBe('false');
});
