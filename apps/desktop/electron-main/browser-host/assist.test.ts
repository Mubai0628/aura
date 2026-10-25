// @vitest-environment node
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { BrowserAssistState, BrowserPageState } from '../../shared/browser-api.js';
import { Assists } from './assist.js';
import type { BrowserPages } from './pages.js';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';

/** The two things a request needs of the pages: their states, and the request shown on one. */
function fakePages() {
  const listeners: ((state: BrowserPageState) => void)[] = [];
  const shown: (BrowserAssistState | undefined)[] = [];
  const pages = {
    onState: (listener: (state: BrowserPageState) => void) => {
      listeners.push(listener);
      return () => undefined;
    },
    setAssist: (_session: string, _page: string, assist: BrowserAssistState | undefined) => {
      shown.push(assist);
    },
  } as unknown as BrowserPages;
  const closed = (sessionId: string, pageId = 'model') => {
    for (const listener of listeners)
      listener({ sessionId, pageId, closed: true } as BrowserPageState);
  };
  return { pages, shown, closed };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("a model's request for the user's help", () => {
  it('stands until the user answers, then is gone', async () => {
    const { pages, shown } = fakePages();
    const assists = new Assists(pages, () => 1_000);
    const waiting = assists.wait(S, 'bcm-1', '请登录测试账号', 60_000);
    expect(shown).toEqual([{ reason: '请登录测试账号', deadline: 61_000 }]);
    expect(assists.answer(S, 'done')).toBe(true);
    await expect(waiting).resolves.toBe('done');
    expect(shown.at(-1)).toBeUndefined();
    // Nothing waits any more: a second press of a button is not an answer.
    expect(assists.answer(S, 'declined')).toBe(false);
  });

  it('ends when its time runs out, when the page closes, or when Core withdraws it', async () => {
    vi.useFakeTimers();
    const { pages, closed } = fakePages();
    const assists = new Assists(pages);
    const timed = assists.wait(S, 'bcm-1', '请完成验证', 30_000);
    vi.advanceTimersByTime(30_000);
    await expect(timed).resolves.toBe('timed_out');

    const page = assists.wait(S, 'bcm-2', '请完成验证', 30_000);
    closed(S, 'tab-1');
    closed(S);
    await expect(page).resolves.toBe('closed');

    const stopped = assists.wait(S, 'bcm-3', '请完成验证', 30_000);
    assists.withdraw('bcm-other');
    assists.withdraw('bcm-3');
    await expect(stopped).resolves.toBe('withdrawn');
    expect(assists.answer(S, 'done')).toBe(false);
  });

  it("is one conversation's own: another conversation's answer does not end it", async () => {
    const { pages } = fakePages();
    const assists = new Assists(pages);
    const waiting = assists.wait(S, 'bcm-1', '请选择文件', 30_000);
    expect(assists.answer('ses_01ARZ3NDEKTSV4RRFFQ69G5FAW', 'done')).toBe(false);
    expect(assists.answer(S, 'declined')).toBe(true);
    await expect(waiting).resolves.toBe('declined');
  });
});
