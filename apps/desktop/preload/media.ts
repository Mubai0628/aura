import { ipcRenderer, webUtils } from 'electron';
import {
  ATTACHMENT_FILE_CHANNEL,
  ATTACHMENT_PICKED_CHANNEL,
  readAttachmentFileResult,
  type AttachmentFileRequest,
  type AttachmentFileResult,
} from '../shared/attachment-files.js';
import {
  MESSAGE_PICTURE_CHANNEL,
  readMessagePictureResult,
  type MessagePictureRequest,
  type MessagePictureResult,
} from '../shared/message-pictures.js';
import {
  WORKSPACE_MEDIA_CHANNEL,
  readWorkspaceMediaResult,
  type WorkspaceMediaRequest,
  type WorkspaceMediaResult,
} from '../shared/workspace-media.js';

/** The address Main streams a workspace's picture, video or sound from (its own channel). */
export async function workspaceMediaUrl(
  request: WorkspaceMediaRequest,
): Promise<WorkspaceMediaResult> {
  return readWorkspaceMediaResult(
    await ipcRenderer.invoke(WORKSPACE_MEDIA_CHANNEL, request).catch(() => undefined),
  );
}

/**
 * Where on this computer a file the user dropped, pasted or picked for a message is, so a
 * file of the workspace can be named by its place (Core checks it is in the conversation's
 * workspace). Empty for one that has none, such as a picture pasted from the clipboard.
 */
export function attachmentPath(file: File): string {
  try {
    const path = webUtils.getPathForFile(file);
    // Main remembers the place of a file the user chose, so it can be viewed before it
    // is sent. Only a file the user picked or dropped has a place; a page cannot make one up.
    if (path) ipcRenderer.send(ATTACHMENT_PICKED_CHANNEL, path);
    return path;
  } catch {
    return '';
  }
}

/** An attached file outside the workspace, streamed, read or opened by Main (its own channel). */
export async function attachmentFile(
  request: AttachmentFileRequest,
): Promise<AttachmentFileResult> {
  return readAttachmentFileResult(
    await ipcRenderer.invoke(ATTACHMENT_FILE_CHANNEL, request).catch(() => undefined),
  );
}

/** A message's picture copied as a picture, or saved where the user chooses (Main reads it). */
export async function messagePicture(
  request: MessagePictureRequest,
): Promise<MessagePictureResult> {
  return readMessagePictureResult(
    await ipcRenderer.invoke(MESSAGE_PICTURE_CHANNEL, request).catch(() => undefined),
  );
}
