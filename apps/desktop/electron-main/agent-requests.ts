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

/**
 * Work whose size the user chose — a folder to add, a plugin or Skill to read and copy — runs in
 * Core for as long as it takes, up to Core's own 120 s bound for workspace commands. Main waits a
 * little longer, so slow work ends as its real result instead of as unconfirmed.
 */
const LONG_WORK_REPLY_BUDGET_MS = 125_000;
const LONG_WORK_COMMANDS: ReadonlySet<string> = new Set([
  'workspace.create',
  'workspace.restore',
  'agent.plugins.preview',
  'agent.plugins.import',
  'agent.skills.preview',
  'agent.skills.import',
]);

/** How long Main waits for Core's reply to one command. */
export function replyBudget(command: string): number {
  if (RUN_STOP_COMMANDS.has(command)) return RUN_STOP_REPLY_BUDGET_MS;
  return LONG_WORK_COMMANDS.has(command) ? LONG_WORK_REPLY_BUDGET_MS : ORDINARY_REPLY_BUDGET_MS;
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
      transport.request(
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
