/**
 * The proof a paired extension gives on every connection: HMAC-SHA256 with the pairing secret
 * over Core's fresh challenge, this extension's id and the protocol version (little-endian), as
 * `aura-chrome-domain::protocol::authentication_mac` computes it. The relay never sees the secret.
 */
import { PROTOCOL_VERSION } from './protocol.js';

export function base64ToBytes(text: string): Uint8Array<ArrayBuffer> {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
  return bytes;
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (const byte of bytes) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export function hex(bytes: Uint8Array): string {
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export async function hmacSha256(
  key: Uint8Array<ArrayBuffer>,
  message: Uint8Array<ArrayBuffer>,
): Promise<Uint8Array> {
  const imported = await crypto.subtle.importKey(
    'raw',
    key,
    { name: 'HMAC', hash: 'SHA-256' },
    false,
    ['sign'],
  );
  return new Uint8Array(await crypto.subtle.sign('HMAC', imported, message));
}

/** Lowercase hex of the proof for `nonce`, as Core expects it. */
export async function authenticationMac(
  secret: Uint8Array<ArrayBuffer>,
  nonce: Uint8Array<ArrayBuffer>,
  extensionId: string,
): Promise<string> {
  const id = new TextEncoder().encode(extensionId);
  const message = new Uint8Array(nonce.length + id.length + 4);
  message.set(nonce, 0);
  message.set(id, nonce.length);
  new DataView(message.buffer).setUint32(nonce.length + id.length, PROTOCOL_VERSION, true);
  return hex(await hmacSha256(secret, message));
}
