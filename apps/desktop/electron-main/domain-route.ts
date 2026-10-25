import type { IpcMainInvokeEvent } from 'electron';

import { mainRoute } from '../shared/generated/routes.js';
import { isGitCommand, isGitParams } from '../shared/git-validation.js';
import { isPackageCommand, isPackageParams } from '../shared/package-validation.js';
import { isMemoryCommand, isMemoryParams } from '../shared/memory-validation.js';
import type { MemoryMainCommand, MemoryMainParams } from '../shared/memory-api.js';
import type { PackageMainCommand, PackageMainParams } from '../shared/package-api.js';
import type { GitMainCommand, GitMainParams } from '../shared/git-api.js';
import {
  isAgentCommand,
  isAgentParams,
  type AgentMainCommand,
  type AgentMainParams,
} from '../shared/agent-validation.js';
import {
  isP03MainCommand,
  isP03MainParams,
  type P03MainCommand,
  type P03MainParams,
} from '../shared/p03-validation.js';
import {
  isP04MainCommand,
  isP04MainParams,
  type P04MainCommand,
  type P04MainParams,
} from '../shared/p04-validation.js';
import {
  isP05MainCommand,
  isP05MainParams,
  isP05SecretMainCommand,
  type P05NonSecretMainCommand,
  type P05MainParams,
} from '../shared/p05-validation.js';
import {
  isP07MainCommand,
  isP07MainParams,
  type P07MainCommand,
  type P07MainParams,
} from '../shared/p07-validation.js';

interface DomainHandlers {
  memory: (
    event: IpcMainInvokeEvent,
    command: MemoryMainCommand,
    params: MemoryMainParams[MemoryMainCommand],
  ) => Promise<unknown>;
  git: (command: GitMainCommand, params: GitMainParams[GitMainCommand]) => Promise<unknown>;
  package: (
    event: IpcMainInvokeEvent,
    command: PackageMainCommand,
    params: PackageMainParams[PackageMainCommand],
  ) => Promise<unknown>;
  agent: (
    event: IpcMainInvokeEvent,
    command: AgentMainCommand,
    params: AgentMainParams[AgentMainCommand],
  ) => Promise<unknown>;
  workspace: (
    event: IpcMainInvokeEvent,
    command: P03MainCommand,
    params: P03MainParams[P03MainCommand],
  ) => Promise<unknown>;
  security: (
    event: IpcMainInvokeEvent,
    command: P04MainCommand,
    params: P04MainParams[P04MainCommand],
  ) => Promise<unknown>;
  model: (
    command: P05NonSecretMainCommand,
    params: P05MainParams[P05NonSecretMainCommand],
  ) => Promise<unknown>;
  tool: (
    event: IpcMainInvokeEvent,
    command: P07MainCommand,
    params: P07MainParams[P07MainCommand],
  ) => Promise<unknown>;
}

export class DomainRouteValidationError extends Error {}

/** Generated ownership selects a domain; domain adapters retain dialog and actor normalization. */
export async function dispatchDomainRoute(
  event: IpcMainInvokeEvent,
  command: string,
  params: unknown,
  handlers: DomainHandlers,
): Promise<unknown> {
  const route = mainRoute(command);
  if (route?.auth !== 'trusted_renderer')
    throw new DomainRouteValidationError('Incorrect route channel');
  switch (route.owner) {
    case 'memory':
      if (isMemoryCommand(command) && isMemoryParams(command, params))
        return handlers.memory(event, command, params);
      break;
    case 'git':
      if (isGitCommand(command) && isGitParams(command, params))
        return handlers.git(command, params);
      break;
    case 'package':
      if (isPackageCommand(command) && isPackageParams(command, params))
        return handlers.package(event, command, params);
      break;
    case 'agent':
      if (isAgentCommand(command) && isAgentParams(command, params))
        return handlers.agent(event, command, params);
      break;
    case 'workspace':
      if (isP03MainCommand(command) && isP03MainParams(command, params))
        return handlers.workspace(event, command, params);
      break;
    case 'security':
      if (isP04MainCommand(command) && isP04MainParams(command, params))
        return handlers.security(event, command, params);
      break;
    case 'model':
      if (
        isP05MainCommand(command) &&
        !isP05SecretMainCommand(command) &&
        isP05MainParams(command, params)
      )
        return handlers.model(command, params);
      break;
    case 'tool':
      if (isP07MainCommand(command) && isP07MainParams(command, params))
        return handlers.tool(event, command, params);
      break;
  }
  throw new DomainRouteValidationError('Invalid domain route parameters');
}
