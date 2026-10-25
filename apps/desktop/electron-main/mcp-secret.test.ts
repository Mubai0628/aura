import { beforeEach, describe, expect, it, vi } from 'vitest';
import { encodeSecretInputFrame } from './protocol.js';
import { requestAgentSecret, type ConfirmedRequest } from './secret-requests.js';
import type { CoreTransport } from './core-transport.js';

const ID = 'crd_01HZY3W5Q9G7X2V4B6N8M0K1P3';

describe('MCP SecretInput', () => {
  it('binds the MCP operation and the key slot and wipes the source', () => {
    const secret = Buffer.from('temporary-mcp-value');
    const frame = encodeSecretInputFrame({
      binding: { channelId: 'chn_01ARZ3NDEKTSV4RRFFQ69G5FAV', connectionEpoch: 1n },
      commandInvocationId: 'civ_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      correlationId: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      operation: 'mcp_secret_set',
      secret,
      sequence: 1n,
    });
    expect(frame[38]).toBe(7);
    expect(frame[71]).toBe(1);
    expect(secret.every((byte) => byte === 0)).toBe(true);
    frame.fill(0);
  });

  describe('request', () => {
    const requestWithSecretInput = vi.fn();
    const confirm: ConfirmedRequest = async ({ accept, send }) =>
      accept(await send({ requestWithSecretInput } as unknown as CoreTransport));
    beforeEach(() => requestWithSecretInput.mockReset());

    it('sends the metadata with the MCP operation and returns only the secret ID', async () => {
      requestWithSecretInput.mockResolvedValue({ secret_id: ID });
      const secret = Buffer.from('another-temporary-value');
      const result = await requestAgentSecret(
        'agent.mcp.secret.set',
        { server_id: 'web', secret_id: null },
        secret,
        confirm,
      );
      expect(result).toEqual({ secret_id: ID });
      const [command, params, operation] = requestWithSecretInput.mock.calls[0] as unknown[];
      expect([command, params, operation]).toEqual([
        'agent.mcp.secret.set',
        { server_id: 'web', secret_id: null },
        'mcp_secret_set',
      ]);
      expect(secret.every((byte) => byte === 0)).toBe(true);
    });

    it('refuses metadata outside the contract before anything is sent', async () => {
      const secret = Buffer.from('a-value-that-never-leaves');
      await expect(
        requestAgentSecret(
          'agent.mcp.secret.set',
          { server_id: 'web', secret_id: null, secret: 'inline' } as never,
          secret,
          confirm,
        ),
      ).rejects.toThrow();
      expect(requestWithSecretInput).not.toHaveBeenCalled();
      expect(secret.every((byte) => byte === 0)).toBe(true);
    });

    it('an answer that is not the contract is an unknown outcome, never a retry', async () => {
      requestWithSecretInput.mockResolvedValue({ secret_id: ID, value: 'leaked' });
      const error: unknown = await requestAgentSecret(
        'agent.mcp.secret.set',
        { server_id: 'web', secret_id: null },
        Buffer.from('x'),
        confirm,
      ).catch((reason: unknown) => reason);
      // errorEnvelope() prefixes Main's codes with `ipc.`.
      expect((error as { envelope?: { code?: string } }).envelope?.code).toBe(
        'ipc.command.outcome_unknown',
      );
    });
  });
});
