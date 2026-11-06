import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';

import { MessageAttachments } from './MessageAttachments';
import { resetPictureReadsForTests } from './picture-reads';

// A long conversation holds many pictures, each a read of its whole bytes from Core. A picture is
// read once it comes near the view, and a picture read once is shown again without another read.

const picture = (index: number): AgentMessageAttachment => ({
  kind: 'image',
  name: `shot-${index}.png`,
  media_type: 'image/png',
  byte_length: 12,
  artifact_id: `art_${index}`,
});

const readScreenshot = vi.fn();
const watched = new Map<Element, IntersectionObserverCallback>();
const original = window.IntersectionObserver;

/** Tells the observer watching `element` that it has come near the view. */
function near(element: Element) {
  const callback = watched.get(element);
  expect(callback).toBeDefined();
  act(() =>
    callback!(
      [{ isIntersecting: true, target: element } as unknown as IntersectionObserverEntry],
      {} as IntersectionObserver,
    ),
  );
}

beforeEach(() => {
  resetPictureReadsForTests();
  watched.clear();
  readScreenshot.mockReset();
  readScreenshot.mockImplementation(async ({ artifact_id }: { artifact_id: string }) => ({
    ok: true,
    value: { media_type: 'image/png', data_base64: `${artifact_id}==` },
  }));
  Object.assign(window, { aura: { v1: { browser: { readScreenshot } } } });
  window.IntersectionObserver = class {
    readonly #callback: IntersectionObserverCallback;
    readonly #targets: Element[] = [];
    constructor(callback: IntersectionObserverCallback) {
      this.#callback = callback;
    }
    observe(element: Element) {
      this.#targets.push(element);
      watched.set(element, this.#callback);
    }
    disconnect() {
      for (const element of this.#targets) watched.delete(element);
    }
  } as unknown as typeof IntersectionObserver;
});
afterEach(() => {
  cleanup();
  window.IntersectionObserver = original;
});

test('a sent picture is read only once it comes near the view', async () => {
  render(<MessageAttachments attachments={[picture(1), picture(2), picture(3)]} sessionId="s" />);
  const items = screen.getAllByRole('listitem');
  expect(items).toHaveLength(3);
  expect(readScreenshot).not.toHaveBeenCalled();
  near(items[1]!);
  expect((await screen.findByRole('img', { name: 'shot-2.png' })).getAttribute('src')).toBe(
    'data:image/png;base64,art_2==',
  );
  expect(readScreenshot).toHaveBeenCalledTimes(1);
  expect(readScreenshot).toHaveBeenCalledWith({ session_id: 's', artifact_id: 'art_2' });
  expect(screen.queryByRole('img', { name: 'shot-1.png' })).toBeNull();
});

test('a picture read once is shown again without another read', async () => {
  const first = render(<MessageAttachments attachments={[picture(4)]} sessionId="s" />);
  near(screen.getByRole('listitem'));
  await screen.findByRole('img', { name: 'shot-4.png' });
  first.unmount();
  render(<MessageAttachments attachments={[picture(4)]} sessionId="s" />);
  near(screen.getByRole('listitem'));
  expect((await screen.findByRole('img', { name: 'shot-4.png' })).getAttribute('src')).toBe(
    'data:image/png;base64,art_4==',
  );
  expect(readScreenshot).toHaveBeenCalledTimes(1);
});

test('a picture that could not be read is asked for again the next time', async () => {
  readScreenshot.mockResolvedValueOnce({ ok: false, error: { code: 'ipc.handler_overloaded' } });
  const first = render(<MessageAttachments attachments={[picture(5)]} sessionId="s" />);
  near(screen.getByRole('listitem'));
  expect((await screen.findByRole('status')).textContent).toBe('图片不可用');
  first.unmount();
  render(<MessageAttachments attachments={[picture(5)]} sessionId="s" />);
  near(screen.getByRole('listitem'));
  await screen.findByRole('img', { name: 'shot-5.png' });
  expect(readScreenshot).toHaveBeenCalledTimes(2);
});
