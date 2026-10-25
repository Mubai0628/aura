import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import { ActivitySession } from './activity-session';
import { ToolActivity } from './ToolActivity';

// The screenshot a browser call took shows under its row in the
// conversation once the row is opened, read from the conversation's own record.

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const IMAGE = 'art_01ARZ3NDEKTSV4RRFFQ69G5FAV';

const shot = (): AgentToolActivityEntry =>
  ({
    display: {
      details: [],
      internal: false,
      summary: '截图 1280×800 · 已发给模型',
      target: 'http://127.0.0.1:9/game',
      title: '网页截图',
      truncated: false,
      image: IMAGE,
    },
    summary: {
      can_cancel: false,
      completed_at: null,
      display_name: 'browser.screenshot',
      ordinal: 1,
      outcome: { type: 'succeeded' },
      side_effect_state: null,
      started_at: null,
      status: 'finalized',
      tool_batch_id: 'tbt_1',
      tool_call_id: 'tcl_01HOSTTOOLCALL0000000001',
      tool_id: 'tol_01',
      tool_version: { definition_revision: '1', semantic_version: null },
    },
  }) as unknown as AgentToolActivityEntry;

const readScreenshot = vi.fn();
beforeEach(() => {
  readScreenshot.mockReset();
  readScreenshot.mockResolvedValue({
    ok: true,
    value: { media_type: 'image/jpeg', data_base64: '/9j/4AAQSkZJRg==' },
  });
  Object.assign(window, { aura: { v1: { browser: { readScreenshot } } } });
});
afterEach(cleanup);

test("a screenshot call's row opens to the picture, read only when opened", async () => {
  const { container } = render(
    <ActivitySession.Provider value={S}>
      <ToolActivity entry={shot()} />
    </ActivitySession.Provider>,
  );
  expect(screen.getByText('网页截图')).toBeTruthy();
  expect(screen.getByText('截图 1280×800 · 已发给模型')).toBeTruthy();
  expect(readScreenshot).not.toHaveBeenCalled();
  const details = container.querySelector('details') as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  const image = await waitFor(() => screen.getByRole('img', { name: '网页截图' }));
  expect(image.getAttribute('src')).toBe('data:image/jpeg;base64,/9j/4AAQSkZJRg==');
  expect(readScreenshot).toHaveBeenCalledWith({ session_id: S, artifact_id: IMAGE });
});

test('a screenshot that can no longer be read says so', async () => {
  readScreenshot.mockResolvedValue({ ok: false, error: { message: 'gone' } });
  const { container } = render(
    <ActivitySession.Provider value={S}>
      <ToolActivity entry={shot()} />
    </ActivitySession.Provider>,
  );
  const details = container.querySelector('details') as HTMLDetailsElement;
  details.open = true;
  fireEvent(details, new Event('toggle'));
  await waitFor(() => expect(screen.getByRole('status').textContent).toBe('截图不可用'));
});
