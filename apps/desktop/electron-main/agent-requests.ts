import type {
  AgentMainCommand,
  AgentMainParams,
  AgentMainResult,
} from '../shared/generated/routes.js';
import type { CorrelationId } from '@aura/contracts/generated';
import { parseAgentResult } from '../shared/agent-validation.js';
import type { ConfirmedRequest } from './secret-requests.js';

const ORDINARY_REPLY_BUDGET_MS = 15_000;

/**
 * These commands wait inside Core for a run's real end, for up to its 45 s stop budget
 * (`RUN_STOP_BUDGET`), and then record the interruption. Main waits past that budget, so a stop
 * that ends normally is reported as one instead of as unconfirmed.
 */
const RUN_STOP_REPLY_BUDGET_MS = 60_000;
const RUN_STOP_COMMANDS: ReadonlySet<string> = new Set([
  'agent.cancel',
  'agent.mode.control',
  'agent.query.answer',
  'agent.approval.resolve',
  'agent.context.recheck',
  'agent.context.window.resolve',
  // A mode change stops running calls the new mode no longer allows.
  'permission.set_session_mode',
]);

/** How long Main waits for Core's reply to one command. */
export function replyBudget(command: string): number {
  return RUN_STOP_COMMANDS.has(command) ? RUN_STOP_REPLY_BUDGET_MS : ORDINARY_REPLY_BUDGET_MS;
}

/** Normal replies and confirmed outcomes use the same Rust-generated result validator. */
export function requestAgentCommand<C extends AgentMainCommand>(
  command: C,
  params: AgentMainParams[C],
  confirm: ConfirmedRequest,
  onRequestId?: (id: CorrelationId) => void,
  onQueuedOpening?: (cancelIfUnsent: () => boolean) => void,
): Promise<AgentMainResult[C]> {
  return confirm({
    accept: (value) => parseAgentResult(command, value),
    command,
    send: (transport) =>
      onRequestId === undefined && onQueuedOpening === undefined && !RUN_STOP_COMMANDS.has(command)
        ? transport.request(command, params)
        : transport.request(
            command,
            params,
            1,
            replyBudget(command),
            undefined,
            onRequestId,
            onQueuedOpening,
          ),
  });
}
