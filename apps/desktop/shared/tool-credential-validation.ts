import { mainRoute } from './generated/routes.js';
import type {
  ToolCredentialMainCommand,
  ToolCredentialMainParams,
  ToolCredentialMainResult,
} from './generated/routes.js';
import {
  validateToolCredentialRequest,
  validateToolCredentialResponse,
} from './generated/tool-credential-validator.js';

export type {
  ToolCredentialMainCommand,
  ToolCredentialMainParams,
  ToolCredentialMainResult,
} from './generated/routes.js';
export type ToolCredentialSecretCommand = Extract<
  ToolCredentialMainCommand,
  'tools.credential.create' | 'tools.credential.rotate'
>;
export type ToolCredentialReadCommand = Exclude<
  ToolCredentialMainCommand,
  ToolCredentialSecretCommand
>;

export function isToolCredentialCommand(value: string): value is ToolCredentialMainCommand {
  return (
    value.startsWith('tools.credential.') &&
    value !== 'tools.credential.candidates' &&
    mainRoute(value)?.owner === 'tool'
  );
}
export function isToolCredentialSecretCommand(value: string): value is ToolCredentialSecretCommand {
  return isToolCredentialCommand(value) && mainRoute(value)?.auth === 'secret_input';
}
export const isToolCredentialParams = <C extends ToolCredentialMainCommand>(
  command: C,
  value: unknown,
): value is ToolCredentialMainParams[C] => validateToolCredentialRequest({ command, value });

export function parseToolCredentialResult<C extends ToolCredentialMainCommand>(
  command: C,
  value: unknown,
): ToolCredentialMainResult[C] {
  if (!validateToolCredentialResponse({ command, value }))
    throw new Error('Invalid tool credential response');
  return value as ToolCredentialMainResult[C];
}
