import { cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { ReplyActions } from './ReplyActions';

// 复制
// copies the whole reply as it was written, read to its end first; a reply that cannot be read
// whole is not half copied.

afterEach(cleanup);

/** A reply of `total` bytes, paged as the Core pages it. */
function pages(total: number, failAt?: number) {
  const text = Array.from({ length: total }, (_, index) =>
    String.fromCharCode(97 + (index % 26)),
  ).join('');
  const readAgentMessageText = vi.fn(async (request: { offset: number; limit: number }) => {
    if (failAt !== undefined && request.offset >= failAt)
      return { ok: false, error: { message: '读取失败' } };
    const end = Math.min(total, request.offset + request.limit);
    return {
      ok: true,
      value: {
        source: { kind: 'assistant', model_step_id: 'step' },
        text: text.slice(request.offset, end),
        total_bytes: total,
        from_offset: request.offset,
        next_offset: end < total ? end : null,
        body_id: null,
      },
    };
  });
  // Main refuses the browser's clipboard to
  // every page, so the text goes to Main.
  const writeClipboard = vi.fn<
    (request: { text: string }) => Promise<{
      ok: true;
      value: { completed: boolean; message: string };
    }>
  >(async () => ({ ok: true, value: { completed: true, message: '已复制' } }));
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText, writeClipboard } },
  });
  return { text, readAgentMessageText, writeClipboard };
}

test('复制 copies the whole reply, read to its end, and says so', async () => {
  const user = userEvent.setup();
  await navigator.clipboard.writeText('the page clipboard is never used');
  const { text, readAgentMessageText, writeClipboard } = pages(150_000);
  render(
    <ReplyActions
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step' }}
      totalBytes={150_000}
    />,
  );
  await user.click(screen.getByRole('button', { name: '复制' }));
  await waitFor(() => expect(screen.getByRole('button', { name: '已复制' })).toBeTruthy());
  expect(writeClipboard).toHaveBeenCalledExactlyOnceWith({ text });
  expect(await navigator.clipboard.readText()).toBe('the page clipboard is never used');
  expect(readAgentMessageText.mock.calls.length).toBeGreaterThan(1);
});

test('what Main refuses is said where the button is', async () => {
  const user = userEvent.setup();
  const { writeClipboard } = pages(10);
  writeClipboard.mockResolvedValueOnce({
    ok: true,
    value: { completed: false, message: '剪贴板暂时不可用（可能正被其他程序占用），请重试。' },
  });
  render(
    <ReplyActions
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step' }}
      totalBytes={10}
    />,
  );
  await user.click(screen.getByRole('button', { name: '复制' }));
  expect(await screen.findByRole('alert')).toHaveProperty(
    'textContent',
    '剪贴板暂时不可用（可能正被其他程序占用），请重试。',
  );
});

test('a reply that cannot be read whole is not half copied', async () => {
  const user = userEvent.setup();
  const { writeClipboard } = pages(150_000, 65_536);
  render(
    <ReplyActions
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step' }}
      totalBytes={150_000}
    />,
  );
  await user.click(screen.getByRole('button', { name: '复制' }));
  expect(await screen.findByRole('alert')).toHaveProperty('textContent', '没能复制回复，请重试。');
  expect(writeClipboard).not.toHaveBeenCalled();
});

test('without a way to branch only 复制 shows; with one, 新建分支 too', () => {
  pages(10);
  const onBranch = vi.fn();
  const { rerender } = render(
    <ReplyActions
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step' }}
      totalBytes={10}
    />,
  );
  expect(screen.queryByRole('button', { name: '新建分支' })).toBeNull();
  rerender(
    <ReplyActions
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step' }}
      totalBytes={10}
      onBranch={onBranch}
    />,
  );
  screen.getByRole('button', { name: '新建分支' }).click();
  expect(onBranch).toHaveBeenCalledOnce();
});
