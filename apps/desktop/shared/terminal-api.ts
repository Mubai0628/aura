import type {
  TerminalOutputBatchV1,
  TerminalViewV1,
  TerminalSensitivePendingV1,
  TerminalSensitiveReplyV1,
  TerminalSensitiveResultV1,
} from '@aura/contracts/generated';
import type { AgentMainParams, AgentMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';
import type { TerminalOpeningApiV1 } from './terminal-opening-api.js';
export const TERMINAL_OUTPUT_CHANNEL = 'aura:v1:terminal-output';
export const TERMINAL_STATE_CHANNEL = 'aura:v1:terminal-state';
export const TERMINAL_SENSITIVE_CHANNEL = 'aura:v1:terminal-sensitive';
type Request<M extends keyof AgentMainParams> = (
  request: AgentMainParams[M],
) => Promise<ResultEnvelope<AgentMainResult[M]>>;
export interface TerminalApiV1 extends TerminalOpeningApiV1 {
  open: Request<'terminal.user.open'>;
  list: Request<'terminal.user.list'>;
  write: Request<'terminal.user.write'>;
  resize: Request<'terminal.user.resize'>;
  close: Request<'terminal.user.close'>;
  subscribe: Request<'terminal.user.subscribe'>;
  control: Request<'terminal.user.control'>;
  pendingSensitive: Request<'terminal.user.sensitive.pending'>;
  cancelSensitive: Request<'terminal.user.sensitive.cancel'>;
  submitSensitive: (
    request: TerminalSensitiveReplyV1,
    secret: Uint8Array,
  ) => Promise<ResultEnvelope<TerminalSensitiveResultV1>>;
  listProcesses: Request<'agent.process.list'>;
  stopProcess: Request<'agent.process.stop'>;
  onSensitive: (listener: (pending: TerminalSensitivePendingV1) => void) => () => void;
  onOutput: (listener: (batches: TerminalOutputBatchV1[]) => void) => () => void;
  onState: (listener: (view: TerminalViewV1) => void) => () => void;
}
