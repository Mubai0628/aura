import {
  isToolCredentialParams,
  isToolCredentialSecretCommand,
  parseToolCredentialResult,
  type ToolCredentialMainCommand,
  type ToolCredentialMainParams,
  type ToolCredentialMainResult,
} from '../shared/tool-credential-validation.js';
import type { CommandInvocationId } from '@aura/contracts/generated';
import type { SecretInputOperation } from './protocol.js';
import { CoreRpcError, errorEnvelope, type CoreTransport } from './core-transport.js';
import {
  isAgentParams,
  parseAgentResult,
  type AgentMainParams,
  type AgentMainResult,
} from '../shared/agent-validation.js';
export type ConfirmedRequest = <T>(options: {
  command: string;
  accept: (value: unknown) => T;
  send: (transport: CoreTransport) => Promise<unknown>;
}) => Promise<T>;
export async function requestToolCredential<C extends ToolCredentialMainCommand>(
  command: C,
  params: ToolCredentialMainParams[C],
  secret: Buffer | undefined,
  supported: boolean,
  confirm: ConfirmedRequest,
): Promise<ToolCredentialMainResult[C]> {
  let invocation: CommandInvocationId | undefined;
  try {
    if (!supported) throw new Error('Core does not support tool credential management.');
    if (
      !isToolCredentialParams(command, params) ||
      isToolCredentialSecretCommand(command) !== (secret !== undefined)
    )
      throw new Error('Invalid tool credential input channel.');
    return await confirm({
      command,
      accept: (value) => {
        try {
          return parseToolCredentialResult(command, value);
        } catch {
          if (command === 'tools.credential.list' || command === 'tools.credential.outcome')
            throw new Error('Invalid tool credential response.');
          throw new CoreRpcError({
            ...errorEnvelope(
              'command.outcome_unknown',
              'Credential response could not be confirmed.',
              'failed',
            ),
            details: {
              outcome_recovery: 'query_runtime.command_outcome_without_replay',
              ...(invocation ? { command_invocation_id: invocation } : {}),
            },
          });
        }
      },
      send: (transport) =>
        secret === undefined
          ? transport.request(command, params, 1, 15_000, (id) => {
              invocation = id;
            })
          : transport.requestWithSecretInput(
              command,
              params,
              command === 'tools.credential.create'
                ? 'tools_credential_create'
                : 'tools_credential_rotate',
              secret,
              45_000,
              (id) => {
                invocation = id;
              },
            ),
    });
  } finally {
    secret?.fill(0);
  }
}

/** The Agent commands that take a secret: terminal input and an MCP server's secret. */
export type AgentSecretCommand = 'terminal.user.sensitive.submit' | 'agent.mcp.secret.set';
const AGENT_SECRET_OPERATION: Record<AgentSecretCommand, SecretInputOperation> = {
  'terminal.user.sensitive.submit': 'terminal_sensitive_input',
  'agent.mcp.secret.set': 'mcp_secret_set',
};

export async function requestAgentSecret<C extends AgentSecretCommand>(
  command: C,
  params: AgentMainParams[C],
  secret: Buffer,
  confirm: ConfirmedRequest,
): Promise<AgentMainResult[C]> {
  let invocation: CommandInvocationId | undefined;
  try {
    if (!isAgentParams(command, params)) throw new Error('Invalid secret input metadata');
    return await confirm({
      command,
      accept: (value) => {
        try {
          return parseAgentResult(command, value);
        } catch {
          throw new CoreRpcError({
            ...errorEnvelope(
              'command.outcome_unknown',
              '安全输入结果未确认；不要重发输入。',
              'failed',
            ),
            details: {
              outcome_recovery: 'query_runtime.command_outcome_without_replay',
              ...(invocation ? { command_invocation_id: invocation } : {}),
            },
          });
        }
      },
      send: (transport) =>
        transport.requestWithSecretInput(
          command,
          params,
          AGENT_SECRET_OPERATION[command],
          secret,
          45000,
          (id) => {
            invocation = id;
          },
        ),
    });
  } finally {
    secret.fill(0);
  }
}
