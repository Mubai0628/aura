/**
 * A
 * picture a message of the conversation carried, copied as a picture or saved where the user
 * chooses. The renderer names it by the conversation and the record; Main reads the picture from
 * Core (only one the conversation may read), never from the page.
 */
export const MESSAGE_PICTURE_CHANNEL = 'aura:v1:message-picture';

export interface MessagePictureRequest {
  action: 'copy' | 'save';
  session_id: string;
  artifact_id: string;
  name: string;
}

export type MessagePictureResult = { completed: boolean; message: string };

const ID = /^[a-z]{3}_[0-9A-HJKMNP-TV-Z]{26}$/u;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

export function parseMessagePictureRequest(value: unknown): MessagePictureRequest | undefined {
  if (!isRecord(value)) return undefined;
  if (Object.keys(value).sort().join(',') !== 'action,artifact_id,name,session_id')
    return undefined;
  const { action, session_id, artifact_id, name } = value;
  return (action === 'copy' || action === 'save') &&
    typeof session_id === 'string' &&
    typeof artifact_id === 'string' &&
    typeof name === 'string' &&
    ID.test(session_id) &&
    session_id.startsWith('ses_') &&
    ID.test(artifact_id) &&
    artifact_id.startsWith('art_') &&
    name.length > 0 &&
    name.length <= 255
    ? { action, session_id, artifact_id, name }
    : undefined;
}

export function readMessagePictureResult(value: unknown): MessagePictureResult {
  return isRecord(value) &&
    typeof value.completed === 'boolean' &&
    typeof value.message === 'string'
    ? { completed: value.completed, message: value.message }
    : { completed: false, message: '没能完成，请重试。' };
}
