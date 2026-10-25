import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, expect, test, vi } from 'vitest';
import type { SessionModelSelectorProjection } from '@aura/contracts/generated';

import { ResizableComposer } from '../ResizableComposer';
import { resetComposerDraftsForTests } from '../agent-session/composer-drafts';
import { MessageAttachments } from './MessageAttachments';
import { ConversationAttachments } from './ConversationAttachments';
import { MAX_TEXT_BYTES, fileKind, readAttachment, typeLabel } from './attachment-files';

// Pictures go in a row of
// thumbnails and files in cards with their type's icon, name, type and size; they are sent with
// the message, which may then have no words of its own.

const selector = {
  binding_revision: '1',
  selected_model_profile_id: 'model',
  models: [],
  endpoints: [],
  providers: [],
} as unknown as SessionModelSelectorProjection;
const submit = vi.fn();
const place = vi.fn();
const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13]);
const encoder = async (file: File) => ({
  mediaType: 'image/png' as const,
  bytes: new Uint8Array(await file.arrayBuffer()),
});

beforeEach(() => {
  localStorage.clear();
  resetComposerDraftsForTests();
  submit.mockReset().mockResolvedValue({ ok: true, value: {} });
  place.mockReset().mockImplementation((file: File) => `C:\\work\\docs\\${file.name}`);
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        listCommands: vi.fn().mockResolvedValue({ ok: true, value: { commands: [] } }),
        listSkills: vi.fn().mockResolvedValue({
          ok: true,
          value: { revision: 1, skills: [], roots: [], issue: null },
        }),
        submitAgentMessage: submit,
        startAgent: vi.fn(),
        attachmentPath: place,
        browser: {
          readScreenshot: vi.fn().mockResolvedValue({
            ok: true,
            value: { media_type: 'image/png', data_base64: 'iVBORw0KGgoAAAAN' },
          }),
        },
      },
    },
  });
  // jsdom has no image decoder: a picture keeps its bytes, as an unscaled PNG does.
  vi.stubGlobal('createImageBitmap', async () => ({ width: 40, height: 30, close: () => {} }));
});
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function renderComposer() {
  render(
    <ConversationAttachments>
      <div aria-label="消息列表" />
      <ResizableComposer
        sessionId="session"
        workspaceId="workspace"
        queuePending={false}
        onChanged={vi.fn()}
        onStop={vi.fn()}
        demoMode={false}
        modelSelector={selector}
        onOpenModelSettings={vi.fn()}
        onOpenPromptInspector={vi.fn()}
        onOpenSecuritySettings={vi.fn()}
        onOpenSettings={vi.fn()}
        onOpenPanel={vi.fn()}
        onSelectModel={vi.fn()}
        onSetPermissionMode={vi.fn()}
        permissionMode="workspace_read_write"
        promptInspector={undefined}
      />
    </ConversationAttachments>,
  );
}

test('a file is read as a picture, a text file with its contents, or a file by its place', async () => {
  const picture = await readAttachment(
    new File([PNG], 'shot.png', { type: 'image/png' }),
    place,
    encoder,
  );
  expect(picture).toMatchObject({
    kind: 'image',
    name: 'shot.png',
    mediaType: 'image/png',
    bytes: 12,
  });
  const notes = await readAttachment(new File(['第一行\nline'], 'notes.md'), place);
  expect(notes).toMatchObject({ kind: 'text', text: '第一行\nline', bytes: 14 });
  const big = await readAttachment(new File(['x'.repeat(MAX_TEXT_BYTES + 1)], 'big.log'), place);
  expect(big).toMatchObject({ kind: 'file', path: 'C:\\work\\docs\\big.log' });
  const binary = await readAttachment(new File(['a\u0000b'], 'data.txt'), place);
  expect(binary).toMatchObject({ kind: 'file' });
  const nowhere = await readAttachment(new File(['%PDF'], 'a.pdf'), () => '');
  expect(nowhere).toEqual({ error: '「a.pdf」没有可用的位置，无法附加。' });
  expect([fileKind('报告.PDF'), fileKind('a.xlsx'), fileKind('a.tsx'), fileKind('noext')]).toEqual([
    'pdf',
    'sheet',
    'code',
    'other',
  ]);
  expect([typeLabel('a.docx'), typeLabel('Makefile')]).toEqual(['DOCX', '文件']);
});

test('pictures show as thumbnails and files as cards, apart, and go with the message', async () => {
  const user = userEvent.setup();
  renderComposer();
  await user.click(screen.getByRole('button', { name: '添加' }));
  await user.click(screen.getByRole('menuitem', { name: '图片或文件' }));
  await user.upload(screen.getByLabelText('选择图片或文件'), [
    new File([PNG], 'shot.png', { type: 'image/png' }),
    new File(['%PDF-1.7'], 'spec.pdf', { type: 'application/pdf' }),
    new File(['buy milk'], 'todo.txt', { type: 'text/plain' }),
  ]);
  const pictures = await screen.findByRole('list', { name: '附加的图片' });
  expect(within(pictures).getByRole('img', { name: 'shot.png' }).getAttribute('src')).toBe(
    'data:image/png;base64,iVBORw0KGgoAAAAN',
  );
  const files = screen.getByRole('list', { name: '附加的文件' });
  const cards = within(files).getAllByRole('listitem');
  expect(cards.map((card) => card.textContent)).toEqual(['spec.pdfPDF · 8 B', 'todo.txtTXT · 8 B']);
  expect(cards[0]?.querySelector('[data-kind="pdf"]')).not.toBeNull();
  expect(cards[0]?.getAttribute('title')).toBe('C:\\work\\docs\\spec.pdf');
  // One taken out again.
  await user.click(within(files).getByRole('button', { name: '移除 todo.txt' }));
  expect(within(files).getAllByRole('listitem')).toHaveLength(1);
  // With no words of its own the message can still be sent.
  const sendButton = screen.getByRole<HTMLButtonElement>('button', { name: '发送消息' });
  expect(sendButton.disabled).toBe(false);
  await user.click(sendButton);
  await waitFor(() => expect(submit).toHaveBeenCalledTimes(1));
  expect(submit.mock.calls[0]?.[0]).toMatchObject({
    session_id: 'session',
    text: '',
    attachments: [
      { kind: 'image', name: 'shot.png', media_type: 'image/png', data_base64: 'iVBORw0KGgoAAAAN' },
      { kind: 'file', path: 'C:\\work\\docs\\spec.pdf' },
    ],
  });
  await waitFor(() => expect(screen.queryByRole('list', { name: '附加的图片' })).toBeNull());
  expect(screen.queryByRole('list', { name: '附加的文件' })).toBeNull();
});

test('a pasted picture is added, and a file that cannot be sent says why', async () => {
  const user = userEvent.setup();
  renderComposer();
  fireEvent.paste(screen.getByLabelText('输入消息'), {
    clipboardData: { files: [new File([PNG], 'image.png', { type: 'image/png' })] },
  });
  expect(await screen.findByRole('img', { name: 'image.png' })).toBeTruthy();
  place.mockReturnValue('');
  await user.upload(screen.getByLabelText('选择图片或文件'), [
    new File(['%PDF'], 'elsewhere.pdf', { type: 'application/pdf' }),
  ]);
  expect((await screen.findByRole('alert')).textContent).toContain(
    '「elsewhere.pdf」没有可用的位置',
  );
});

test('a composer thumbnail opens a focused preview, zooms and closes back to its trigger', async () => {
  const user = userEvent.setup();
  renderComposer();
  await user.upload(screen.getByLabelText('选择图片或文件'), [
    new File([PNG], 'shot.png', { type: 'image/png' }),
  ]);
  const trigger = await screen.findByRole('button', { name: '预览 shot.png' });
  await user.click(trigger);
  const preview = screen.getByRole('dialog', { name: '图片预览' });
  const image = within(preview).getByRole('img', { name: 'shot.png' });
  expect(document.activeElement).toBe(
    within(preview).getByRole('button', { name: '关闭图片预览' }),
  );
  fireEvent.wheel(preview, { deltaY: -100, clientX: 100, clientY: 80 });
  expect(image.style.transform).not.toContain('scale(1)');
  await user.keyboard('{Escape}');
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(document.activeElement).toBe(trigger);
  await user.click(screen.getByRole('button', { name: '移除 shot.png' }));
  expect(screen.queryByRole('dialog')).toBeNull();
  expect(screen.queryByRole('img', { name: 'shot.png' })).toBeNull();
});

test('a message picture opens the same preview', async () => {
  const user = userEvent.setup();
  render(
    <MessageAttachments
      sessionId="session"
      attachments={[
        {
          kind: 'image',
          name: 'sent.png',
          media_type: 'image/png',
          byte_length: 12,
          artifact_id: 'art_sent',
        },
      ]}
    />,
  );
  await user.click(await screen.findByRole('button', { name: '预览 sent.png' }));
  expect(
    within(screen.getByRole('dialog', { name: '图片预览' })).getByRole('img', { name: 'sent.png' }),
  ).toBeTruthy();
  await user.click(screen.getByRole('button', { name: '关闭图片预览' }));
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('dropping files on the conversation list adds them once to the composer; text is ignored', async () => {
  renderComposer();
  const list = screen.getByLabelText('消息列表');
  const dataTransfer = {
    types: ['Files'],
    files: [new File(['%PDF'], 'dropped.pdf', { type: 'application/pdf' })],
  };
  fireEvent.dragEnter(list, { dataTransfer });
  expect(
    screen.getByRole('region', { name: '当前会话' }).classList.contains('is-file-drop-target'),
  ).toBe(true);
  fireEvent.drop(list, { dataTransfer });
  expect(await screen.findByText('dropped.pdf')).toBeTruthy();
  expect(screen.getAllByText('dropped.pdf')).toHaveLength(1);
  expect(
    screen.getByRole('region', { name: '当前会话' }).classList.contains('is-file-drop-target'),
  ).toBe(false);
  expect(fireEvent.drop(list, { dataTransfer: { types: ['text/plain'], files: [] } })).toBe(true);
  expect(screen.getAllByRole('listitem')).toHaveLength(1);
});

test('the conversation shows a message’s pictures and files under it', async () => {
  render(
    <MessageAttachments
      sessionId="session"
      attachments={[
        {
          kind: 'image',
          name: 'shot.png',
          media_type: 'image/png',
          byte_length: 12,
          artifact_id: 'art_1',
        },
        { kind: 'file', name: 'spec.pdf', path: 'docs/spec.pdf', byte_length: 2_411_724 },
        { kind: 'text', name: 'notes.md', byte_length: 2048, artifact_id: 'art_2' },
      ]}
    />,
  );
  expect((await screen.findByRole('img', { name: 'shot.png' })).getAttribute('src')).toBe(
    'data:image/png;base64,iVBORw0KGgoAAAAN',
  );
  const cards = within(screen.getByRole('list', { name: '发送的文件' })).getAllByRole('listitem');
  expect(cards.map((card) => card.textContent)).toEqual([
    'spec.pdfPDF · 2.3 MB',
    'notes.mdMD · 2 KB',
  ]);
  expect(cards[0]?.getAttribute('title')).toBe('docs/spec.pdf');
  expect(screen.queryByRole('button', { name: /移除/u })).toBeNull();
});
