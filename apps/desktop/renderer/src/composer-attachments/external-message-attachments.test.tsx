import type { AgentMessageAttachment } from '@aura/contracts/generated';
import { cleanup, render, screen, within } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';

import { MessageAttachments } from './MessageAttachments';
import { attachmentDrafts, MAX_TEXT_BYTES, readAttachment } from './attachment-files';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

// Saved-message JSON checks the Renderer consumer independently of compile-time narrowing.
const fromJson = (value: unknown) => JSON.parse(JSON.stringify(value)) as AgentMessageAttachment[];
const external = [
  {
    kind: 'external_file',
    name: 'clip.mp4',
    path: 'C:\\Users\\Owner\\Downloads\\clip.mp4',
    byte_length: 2048,
    external_grant_id: 'grant_video',
  },
  {
    kind: 'external_file',
    name: 'archive.zip',
    path: 'D:\\Inbox\\archive.zip',
    byte_length: 4096,
    external_grant_id: 'grant_zip',
    listing_artifact_id: 'artifact_zip_listing',
  },
];
const card = (name: string) => screen.getByText(name).closest('li')!;

test('an external attachment is a file card whose title is its complete location', () => {
  render(<MessageAttachments sessionId="session" attachments={fromJson(external)} />);
  expect(card('clip.mp4').getAttribute('title')).toBe(external[0]!.path);
  expect(card('archive.zip').getAttribute('title')).toBe(external[1]!.path);
  expect(within(card('clip.mp4')).getByText('MP4 · 2 KB')).toBeTruthy();
  expect(card('archive.zip').querySelector('[data-kind="archive"]')).not.toBeNull();
});

test('external file cards keep their identities when reordered or their zip listing is refreshed', () => {
  const errors = vi.spyOn(console, 'error').mockImplementation(() => {});
  const rendered = render(
    <MessageAttachments sessionId="session" attachments={fromJson(external)} />,
  );
  const originalVideo = card('clip.mp4');
  const originalZip = card('archive.zip');
  rendered.rerender(
    <MessageAttachments sessionId="session" attachments={fromJson([...external].reverse())} />,
  );
  expect(card('clip.mp4')).toBe(originalVideo);
  expect(card('archive.zip')).toBe(originalZip);
  rendered.rerender(
    <MessageAttachments
      sessionId="session"
      attachments={fromJson([
        external[0],
        { ...external[1], listing_artifact_id: 'artifact_zip_listing_2' },
      ])}
    />,
  );
  expect(card('archive.zip')).toBe(originalZip);
  expect(errors.mock.calls.some((args) => String(args[0]).includes('unique "key"'))).toBe(false);
});

test('outside-workspace non-text and large text remain location-only drafts without reading contents', async () => {
  const video = new File(['not decoded here'], 'clip.mp4', { type: 'video/mp4' });
  const large = new File(['x'.repeat(MAX_TEXT_BYTES + 1)], 'big.txt', { type: 'text/plain' });
  const videoRead = vi.spyOn(video, 'arrayBuffer');
  const largeRead = vi.spyOn(large, 'text');
  const place = (file: File) => `C:\\Users\\Owner\\Downloads\\${file.name}`;
  const attachments = await Promise.all([
    readAttachment(video, place),
    readAttachment(large, place),
  ]);
  expect(attachments).toEqual([
    expect.objectContaining({ kind: 'file', path: place(video) }),
    expect.objectContaining({ kind: 'file', path: place(large) }),
  ]);
  const accepted = attachments.map((attachment) => {
    if ('error' in attachment) throw new Error(attachment.error);
    return attachment;
  });
  expect(attachmentDrafts(accepted)).toEqual([
    { kind: 'file', path: place(video) },
    { kind: 'file', path: place(large) },
  ]);
  expect(videoRead).not.toHaveBeenCalled();
  expect(largeRead).not.toHaveBeenCalled();
});

test('small text outside the workspace still carries its content instead of requiring a location', async () => {
  const nowhere = await readAttachment(
    new File(['外部备注'], 'notes.md'),
    vi.fn(() => ''),
  );
  expect(nowhere).toMatchObject({ kind: 'text', text: '外部备注' });
  expect(nowhere).not.toHaveProperty('path');
  // Where it is, when it has a place, so its card can open it.
  const placed = await readAttachment(
    new File(['外部备注'], 'notes.md'),
    vi.fn(() => 'C:\\Users\\me\\notes.md'),
  );
  expect(placed).toMatchObject({ kind: 'text', text: '外部备注', path: 'C:\\Users\\me\\notes.md' });
});
