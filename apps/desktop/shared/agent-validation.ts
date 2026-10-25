import { mainRoute } from './generated/routes.js';
import type { AgentMainCommand, AgentMainParams, AgentMainResult } from './generated/routes.js';
import { validateAgentRequest, validateAgentResponse } from './generated/agent-validator.js';
export type { AgentMainCommand, AgentMainParams, AgentMainResult } from './generated/routes.js';

export const isAgentCommand = (value: string): value is AgentMainCommand =>
  mainRoute(value)?.owner === 'agent';
export const isAgentParams = <C extends AgentMainCommand>(
  command: C,
  value: unknown,
): value is AgentMainParams[C] => validateAgentRequest({ command, value });
export function parseAgentResult<C extends AgentMainCommand>(
  command: C,
  value: unknown,
): AgentMainResult[C] {
  if (!validateAgentResponse({ command, value })) throw new Error('Invalid Agent result contract');
  return value as AgentMainResult[C];
}
export { validateAgentRequest, validateAgentResponse };
