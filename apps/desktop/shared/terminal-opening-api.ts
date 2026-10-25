import type {
  CancelDisposition,
  TerminalOpeningStartV1,
  TerminalOpeningRefV1,
  TerminalOpeningChangeV1,
  TerminalViewV1,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from './aura-api.js';
export const TERMINAL_OPENING_START_CHANNEL = 'aura:v1:terminal-opening-start';
export const TERMINAL_OPENING_CANCEL_CHANNEL = 'aura:v1:terminal-opening-cancel';
export const TERMINAL_OPENING_EVENT_CHANNEL = 'aura:v1:terminal-opening';
export interface TerminalOpeningApiV1 {
  beginOpen(request: TerminalOpeningStartV1): Promise<ResultEnvelope<TerminalViewV1>>;
  cancelOpen(request: TerminalOpeningRefV1): Promise<ResultEnvelope<CancelDisposition>>;
  onOpening(listener: (change: TerminalOpeningChangeV1) => void): () => void;
}
