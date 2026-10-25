import type {
  ConfigRecoveryMainCommand,
  ConfigRecoveryMainParams,
  ConfigRecoveryMainResult,
} from './generated/routes.js';
import { mainRoute } from './generated/routes.js';
import {
  validateRecoveryRequest,
  validateRecoveryResponse,
} from './generated/recovery-validator.js';

export type {
  ConfigRecoveryMainCommand,
  ConfigRecoveryMainParams,
  ConfigRecoveryMainResult,
} from './generated/routes.js';

export const isRecoveryCommand = (command: string): command is ConfigRecoveryMainCommand =>
  command.startsWith('config.recovery.') && mainRoute(command)?.auth === 'trusted_renderer';

export const isRecoveryParams = <Command extends ConfigRecoveryMainCommand>(
  command: Command,
  value: unknown,
): value is ConfigRecoveryMainParams[Command] => validateRecoveryRequest({ command, value });

export function parseRecoveryResult<Command extends ConfigRecoveryMainCommand>(
  command: Command,
  value: unknown,
): ConfigRecoveryMainResult[Command] {
  if (!validateRecoveryResponse({ command, value }))
    throw new Error('Invalid configuration recovery response');
  return value as ConfigRecoveryMainResult[Command];
}
