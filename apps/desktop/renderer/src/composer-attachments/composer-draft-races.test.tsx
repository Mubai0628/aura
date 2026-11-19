import { act, renderHook } from '@testing-library/react';
import { beforeEach, expect, test, vi } from 'vitest';
import {
  createLocalConversation,
  discardEmptyConversations,
  getComposerDraft,
  resetComposerDraftsForTests,
} from '../agent-session/composer-drafts';
import { resetKeptDraftsForTests } from '../agent-session/kept-draft';
import type { PictureEncoder } from './attachment-files';
import { useComposerAttachments } from './useComposerAttachments';

const picture = (name: string) => new File(['picture'], name, { type: 'image/png' });
const encoded = { mediaType: 'image/png', bytes: new Uint8Array([1, 2, 3]) } as const;

function imports() {
  const pending = new Map<string, (value: typeof encoded) => void>();
  const encode: PictureEncoder = (file) =>
    new Promise((resolve) => pending.set(file.name, resolve));
  const id = createLocalConversation('workspace');
  const hook = renderHook(() => useComposerAttachments(encode, id));
  return { ...hook, id, finish: (name: string) => pending.get(name)!(encoded), encode };
}

beforeEach(() => {
  vi.stubGlobal('aura', { v1: { attachmentPath: () => '' } });
  localStorage.clear();
  resetComposerDraftsForTests();
  resetKeptDraftsForTests();
});

test('simultaneous imports merge without restoring a previously removed image', async () => {
  const { result, finish } = imports();
  await act(async () => {
    const seed = result.current.add([picture('seed.png')]);
    finish('seed.png');
    await seed;
  });
  await act(async () => {
    const first = result.current.add([picture('first.png')]);
    const second = result.current.add([picture('second.png')]);
    result.current.remove(result.current.attachments[0]!.id);
    finish('second.png');
    await second;
    finish('first.png');
    await first;
  });
  expect(result.current.attachments.map(({ name }) => name).sort()).toEqual([
    'first.png',
    'second.png',
  ]);
});

test('switching away during an import preserves the empty conversation and its imported file', async () => {
  const { id, encode, result, finish, unmount } = imports();
  let pending!: Promise<void>;
  act(() => {
    pending = result.current.add([picture('pasted.png')]);
    discardEmptyConversations();
  });
  unmount();
  expect(getComposerDraft(id)).toBeDefined();
  finish('pasted.png');
  await pending;
  const reopened = renderHook(() => useComposerAttachments(encode, id));
  expect(reopened.result.current.attachments.map(({ name }) => name)).toEqual(['pasted.png']);
});

test('a cleared composer cannot receive a late import from the previous message', async () => {
  const { result, finish } = imports();
  await act(async () => {
    const pending = result.current.add([picture('late.png')]);
    result.current.clear();
    finish('late.png');
    await pending;
  });
  expect(result.current.attachments).toEqual([]);
});
