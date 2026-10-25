import type { AgentModeEpisodeView } from '@aura/contracts/generated';
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { ActivityNoteStack } from './ActivityNoteStack';

// Review regressions: synthetic projections only; no real session or credentials.
let viewportWidth = 1800;
let conversationRight = 1300;

function rect(width: number): DOMRect {
  return {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    right: width,
    bottom: 100,
    width,
    height: 100,
    toJSON: () => ({}),
  };
}

beforeEach(() => {
  viewportWidth = 1800;
  conversationRight = 1300;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains('activity-stack')) return rect(306);
    if (this.classList.contains('review-viewport')) return rect(viewportWidth);
    if (this.classList.contains('review-conversation')) return rect(conversationRight);
    return rect(0);
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function episode(session_id: string, max_tokens: number): AgentModeEpisodeView {
  return {
    session_id,
    revision: '3',
    mode: 'goal',
    paused: false,
    goal_budget: { max_tokens, max_turns: 8 },
    inherited_goal: null,
  };
}

function harness() {
  const conversationColumnRef = createRef<HTMLDivElement>();
  const viewportRef = createRef<HTMLDivElement>();
  return (value: AgentModeEpisodeView, onControl = vi.fn().mockResolvedValue(undefined)) => (
    <div key={value.session_id} className="review-viewport" ref={viewportRef}>
      <div className="review-conversation" ref={conversationColumnRef} />
      <ActivityNoteStack
        combinedSidebarWidthRatio={0.22}
        conversationColumnRef={conversationColumnRef}
        viewportRef={viewportRef}
        episode={value}
        onControl={onControl}
      />
    </div>
  );
}

test('mode termination stays reachable in a 1380px window with a 304px sidebar', () => {
  viewportWidth = 1076;
  conversationRight = (1076 + 900) / 2;
  render(harness()(episode('session-a', 2000)));
  // 1076 - 12 - 988 = 76px cannot hold the card, so a compact entry states the
  // current mode; opening it exposes the real mode controls instead of hiding
  // them, which is what the review asked for.
  const entry = screen.getByRole('button', { name: /展开近期活动（当前模式 Goal）/u });
  expect(entry.textContent).toContain('Goal');
  fireEvent.click(entry);
  expect(screen.queryByRole('button', { name: '终止' })).not.toBeNull();
});

test('Goal budget drafts cannot be submitted to another session with the same revision', async () => {
  const ui = harness();
  const controlA = vi.fn().mockResolvedValue(undefined);
  const controlB = vi.fn().mockResolvedValue(undefined);
  const { rerender } = render(ui(episode('session-a', 2000), controlA));
  fireEvent.click(screen.getByRole('button', { name: '编辑额度' }));
  fireEvent.change(screen.getByRole('spinbutton', { name: 'Token 上限' }), {
    target: { value: '8888' },
  });
  rerender(ui(episode('session-b', 9000), controlB));
  const leakedSave = screen.queryByRole('button', { name: '保存额度' });
  if (leakedSave) await act(async () => fireEvent.click(leakedSave));
  expect(controlB).not.toHaveBeenCalled();
});
