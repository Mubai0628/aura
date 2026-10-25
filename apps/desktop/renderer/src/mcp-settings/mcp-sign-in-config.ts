/** The sign-in parts of a server's configuration, built from the form. */
import type { McpHttpAuth, McpSecretEnvironment, McpServerConfig } from '@aura/contracts/generated';
import type { McpSecretInputHandle } from './McpSecretInput';

export type HttpMode = 'none' | 'bearer' | 'header' | 'oauth';
export interface SecretRow {
  key: number;
  name: string;
  secretId: string | undefined;
}

/** A secret not written yet is checked under this valid placeholder ID. */
const PREVIEW_ID = 'crd_00000000000000000000000000';

const authOf = (config?: McpServerConfig) =>
  config?.transport.type === 'http' ? (config.transport.auth ?? undefined) : undefined;

export const initialMode = (config?: McpServerConfig): HttpMode => {
  const auth = authOf(config);
  if (auth?.type === 'oauth') return 'oauth';
  return auth?.type === 'secret' ? (auth.header ? 'header' : 'bearer') : 'none';
};
export const initialClientId = (config?: McpServerConfig): string => {
  const auth = authOf(config);
  return auth?.type === 'oauth' ? (auth.client_id ?? '') : '';
};
export const initialScopes = (config?: McpServerConfig): string => {
  const auth = authOf(config);
  return auth?.type === 'oauth' ? (auth.scopes ?? []).join(' ') : '';
};

/** OAuth: an optional client ID (Aura registers itself without one) and optional scopes. */
export function oauthAuth(clientId: string, scopes: string): McpHttpAuth {
  const client_id = clientId.trim();
  const listed = scopes.split(/\s+/u).filter(Boolean);
  return {
    type: 'oauth',
    ...(client_id ? { client_id } : {}),
    ...(listed.length ? { scopes: listed } : {}),
  };
}
export const initialHeader = (config?: McpServerConfig): string => {
  const auth = authOf(config);
  return auth?.type === 'secret' ? (auth.header ?? '') : '';
};
export const savedAuthSecret = (config?: McpServerConfig): string | undefined => {
  const auth = authOf(config);
  return auth?.type === 'secret' ? auth.secret_id : undefined;
};
export const initialRows = (config?: McpServerConfig): SecretRow[] =>
  config?.transport.type === 'stdio'
    ? (config.transport.secret_environment ?? []).map((entry, key) => ({
        key,
        name: entry.name,
        secretId: entry.secret_id,
      }))
    : [];

export function httpAuth(
  mode: Exclude<HttpMode, 'none' | 'oauth'>,
  header: string,
  secretId: string | undefined,
): McpHttpAuth {
  const secret_id = secretId ?? PREVIEW_ID;
  return mode === 'header'
    ? { type: 'secret', secret_id, header: header.trim() }
    : { type: 'secret', secret_id };
}

export const secretEnvironment = (rows: SecretRow[]): McpSecretEnvironment[] =>
  rows.map((row) => ({ name: row.name.trim(), secret_id: row.secretId ?? PREVIEW_ID }));

/**
 * Sends a typed secret on the secret channel and returns the ID the configuration names it
 * by; with nothing typed the saved secret stays. A secret the server needs but never got is
 * the user's to fill in.
 */
export async function writeSecret(
  serverId: string,
  input: McpSecretInputHandle | null | undefined,
  existing: string | undefined,
  label: string,
): Promise<string> {
  const bytes = input?.takeBytes();
  if (!bytes) {
    if (existing) return existing;
    throw new Error(`mcp.secret_missing: 请填写${label}`);
  }
  const result = await window.aura.v1.setMcpSecret(
    { server_id: serverId, secret_id: existing ?? null },
    bytes,
  );
  if (!result.ok) throw new Error(result.error.message);
  return result.value.secret_id;
}
