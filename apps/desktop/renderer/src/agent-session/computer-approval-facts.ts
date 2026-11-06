import type { ExecutionApprovalView } from '@aura/contracts/generated';

/** The program a computer call operates, by its id and the name the Start menu shows. */
export interface ComputerFacts {
  appId: string;
  name: string;
}

/** The program a `computer.*` call names: its resource, and its name from the call as resolved. */
export function computerFacts(view: ExecutionApprovalView): ComputerFacts | undefined {
  const content = view.operation.content;
  if (!content.action_kind.startsWith('computer.')) return undefined;
  const resource = content.resources.find((one) => one.scope_kind === 'desktop_app');
  if (resource?.scope_kind !== 'desktop_app') return undefined;
  // The resolved call is the host's command: one program, or the one a screenshot shows.
  const command = (content.action as { command?: Record<string, unknown> }).command ?? {};
  const shown = Array.isArray(command.show) ? (command.show[0] as unknown) : undefined;
  const app = (command.app ?? shown) as { id?: unknown; name?: unknown } | undefined;
  const name = typeof app?.name === 'string' && app.id === resource.app_id ? app.name : '';
  return { appId: resource.app_id, name: name || resource.app_id };
}
