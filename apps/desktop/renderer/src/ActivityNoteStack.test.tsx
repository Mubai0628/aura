import { createRef } from 'react';
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';

import { ActivityNoteStack } from './ActivityNoteStack';
import { closeTodo, todoListKey } from './todo/todo-memory';
import type { AgentModeEpisodeView, TodoState } from '@aura/contracts/generated';

const createRect = (width: number): DOMRect => ({
  bottom: 100,
  height: 100,
  left: 0,
  right: width,
  toJSON: () => ({}),
  top: 0,
  width,
  x: 0,
  y: 0,
});

let activityWidth = 240;
let conversationRight = 400;
let viewportWidth = 1_000;
let triggerResize: (() => void) | undefined;

class ResizeObserverStub {
  readonly callback: ResizeObserverCallback;

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    triggerResize = () => callback([], this as unknown as ResizeObserver);
  }

  disconnect() {}

  observe() {}

  unobserve() {}
}

beforeEach(() => {
  activityWidth = 240;
  conversationRight = 400;
  viewportWidth = 1_000;
  triggerResize = undefined;
  vi.stubGlobal('ResizeObserver', ResizeObserverStub);
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: HTMLElement,
  ) {
    if (this.classList.contains('activity-stack')) return createRect(activityWidth);
    if (this.classList.contains('activity-test-conversation')) {
      return createRect(conversationRight);
    }
    if (this.classList.contains('activity-test-viewport')) return createRect(viewportWidth);
    return createRect(0);
  });
});

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

describe('ActivityNoteStack', () => {
  const episode = {
    session_id: 'session',
    revision: '3',
    mode: 'plan',
    paused: false,
    goal_budget: null,
    inherited_goal: null,
    disposition_required: false,
    end_reason: null,
  } as unknown as AgentModeEpisodeView;

  const showCard = (
    combinedSidebarWidthRatio = 0.2,
    extra: Partial<Parameters<typeof ActivityNoteStack>[0]> = {},
  ) => {
    const conversationColumnRef = createRef<HTMLDivElement>();
    const viewportRef = createRef<HTMLDivElement>();
    const view = render(
      <div className="activity-test-viewport" ref={viewportRef}>
        <div className="activity-test-conversation" ref={conversationColumnRef} />
        <ActivityNoteStack
          combinedSidebarWidthRatio={combinedSidebarWidthRatio}
          conversationColumnRef={conversationColumnRef}
          viewportRef={viewportRef}
          episode={episode}
          {...extra}
        />
      </div>,
    );
    const activity = view.container.querySelector<HTMLElement>('.activity-stack');
    return { activity, view };
  };

  test('a wide window shows the card sized from the real available space', () => {
    // 1000 - 12 - 400 = 588 available, so the card takes its comfortable maximum.
    const { activity } = showCard();
    expect(activity?.dataset.display).toBe('full');
    expect(activity?.hidden).toBe(false);
    expect(activity?.style.width).toBe('320px');

    conversationRight = 688;
    act(() => triggerResize?.());
    expect(activity?.style.width).toBe('300px');
  });

  test('a narrow window keeps a compact mode entry that expands the full card', async () => {
    conversationRight = 900;
    const { activity } = showCard();
    // 1000 - 12 - 900 = 88: the card cannot hold its content, so it collapses.
    expect(activity?.dataset.display).toBe('compact');
    expect(activity?.hidden).toBe(true);
    const entry = screen.getByRole('button', { name: /展开近期活动（当前模式 Plan）/u });
    expect(entry.getAttribute('aria-expanded')).toBe('false');

    fireEvent.click(entry);
    expect(activity?.hidden).toBe(false);
    expect(activity?.dataset.expanded).toBe('true');
    // One control at a time: the entry is gone and the card owns the close button.
    expect(screen.queryByRole('button', { name: /展开近期活动/u })).toBeNull();
    expect(screen.getByRole('button', { name: '收起卡片' }).getAttribute('aria-expanded')).toBe(
      'true',
    );
    // The expanded card stays reachable and scrolls its own content.
    expect(activity?.querySelector('.activity-mode-actions')).not.toBeNull();

    fireEvent.keyDown(window, { key: 'Escape' });
    await waitFor(() => expect(activity?.hidden).toBe(true));
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /展开近期活动/u }));
  });

  test('collapsing from inside the card returns focus to the entry', async () => {
    conversationRight = 900;
    const { activity } = showCard();
    fireEvent.click(screen.getByRole('button', { name: /展开近期活动/u }));
    fireEvent.click(screen.getByRole('button', { name: '收起卡片' }));
    await waitFor(() => expect(activity?.hidden).toBe(true));
    // The entry is rendered again for the collapsed card and takes the focus.
    expect(document.activeElement).toBe(screen.getByRole('button', { name: /展开近期活动/u }));
  });

  test('the collapsed card says how far the task list has got, until the list is closed', () => {
    conversationRight = 900;
    const list = {
      turn_task_id: 'trn_progress',
      revision: '2',
      items: [
        { id: 'a', title: '读需求', status: 'completed' },
        { id: 'b', title: '写代码', status: 'in_progress' },
        { id: 'c', title: '测试', status: 'pending' },
      ],
    } as unknown as TodoState;
    showCard(0.2, { taskList: list });
    const entry = screen.getByRole('button', { name: '展开近期活动（当前模式 Plan，待办 1/3）' });
    expect(entry.textContent).toContain('待办 1/3');
    cleanup();
    closeTodo(todoListKey('trn_progress', '2'));
    showCard(0.2, { taskList: list });
    expect(screen.getByRole('button', { name: '展开近期活动（当前模式 Plan）' })).not.toBeNull();
  });

  test('wide sidebars start compact before any layout measurement', () => {
    const { activity } = showCard(0.35);
    expect(activity?.dataset.display).toBe('compact');
  });

  test('the task list is a part of the card, after the conversation', () => {
    const conversationColumnRef = createRef<HTMLDivElement>();
    const viewportRef = createRef<HTMLDivElement>();
    render(
      <div className="activity-test-viewport" ref={viewportRef}>
        <div className="activity-test-conversation" ref={conversationColumnRef} />
        <ActivityNoteStack
          combinedSidebarWidthRatio={0.2}
          conversationColumnRef={conversationColumnRef}
          viewportRef={viewportRef}
          episode={episode}
          todo={<section aria-label="待办">清单</section>}
        />
      </div>,
    );
    const card = screen.getByRole('complementary', { name: '近期活动' });
    const parts = [...card.querySelectorAll('section')].map((part) =>
      part.getAttribute('aria-label'),
    );
    expect(parts).toEqual(['会话', '待办']);
  });

  test('no mode episode means no card and no placeholder activity', () => {
    const conversationColumnRef = createRef<HTMLDivElement>();
    const viewportRef = createRef<HTMLDivElement>();
    const { container } = render(
      <div className="activity-test-viewport" ref={viewportRef}>
        <div className="activity-test-conversation" ref={conversationColumnRef} />
        <ActivityNoteStack
          combinedSidebarWidthRatio={0.2}
          conversationColumnRef={conversationColumnRef}
          viewportRef={viewportRef}
        />
      </div>,
    );
    expect(container.querySelector('.activity-stack')).toBeNull();
    expect(screen.queryByText('演示')).toBeNull();
  });

  test('shows Plan terminate without Goal pause or budget edit', () => {
    const conversationColumnRef = createRef<HTMLDivElement>();
    const viewportRef = createRef<HTMLDivElement>();
    const onControl = vi.fn();
    const { container } = render(
      <div className="activity-test-viewport" ref={viewportRef}>
        <div className="activity-test-conversation" ref={conversationColumnRef} />
        <ActivityNoteStack
          combinedSidebarWidthRatio={0.2}
          conversationColumnRef={conversationColumnRef}
          viewportRef={viewportRef}
          episode={{
            session_id: 'session',
            revision: '2',
            mode: 'plan',
            paused: false,
            goal_budget: null,
            inherited_goal: null,
          }}
          onControl={onControl}
        />
      </div>,
    );
    expect(container.querySelector<HTMLElement>('.activity-stack')?.dataset.mode).toBe('plan');
    expect(screen.getByRole('button', { name: '终止' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '暂停' })).toBeNull();
    expect(screen.queryByRole('button', { name: '编辑额度' })).toBeNull();
  });

  test('Goal pause and budget edit do not appear for Normal', () => {
    const conversationColumnRef = createRef<HTMLDivElement>();
    const viewportRef = createRef<HTMLDivElement>();
    const { container } = render(
      <div className="activity-test-viewport" ref={viewportRef}>
        <div className="activity-test-conversation" ref={conversationColumnRef} />
        <ActivityNoteStack
          combinedSidebarWidthRatio={0.2}
          conversationColumnRef={conversationColumnRef}
          viewportRef={viewportRef}
          episode={{
            session_id: 'session',
            revision: '1',
            mode: 'normal',
            paused: false,
            goal_budget: null,
            inherited_goal: null,
          }}
        />
      </div>,
    );
    expect(container.querySelector<HTMLElement>('.activity-stack')?.dataset.mode).toBe('normal');
    expect(screen.queryByRole('button', { name: '终止' })).toBeNull();
    expect(screen.queryByRole('button', { name: '暂停' })).toBeNull();
  });

  test('Goal exposes terminate, pause, and budget edit', () => {
    const conversationColumnRef = createRef<HTMLDivElement>();
    const viewportRef = createRef<HTMLDivElement>();
    render(
      <div className="activity-test-viewport" ref={viewportRef}>
        <div className="activity-test-conversation" ref={conversationColumnRef} />
        <ActivityNoteStack
          combinedSidebarWidthRatio={0.2}
          conversationColumnRef={conversationColumnRef}
          viewportRef={viewportRef}
          episode={{
            session_id: 'session',
            revision: '3',
            mode: 'goal',
            paused: false,
            goal_budget: { max_tokens: 2000, max_turns: 8 },
            inherited_goal: {
              budget: { max_tokens: 2000, max_turns: 8 },
              completed: false,
              consumed_tokens: 400,
              consumed_turns: 2,
              revision: '3',
              turn_task_id: 'turn',
            },
          }}
        />
      </div>,
    );
    expect(screen.getByRole('button', { name: '终止' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '暂停' })).toBeTruthy();
    expect(screen.getByRole('button', { name: '编辑额度' })).toBeTruthy();
  });
});
