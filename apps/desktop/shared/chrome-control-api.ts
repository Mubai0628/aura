import type { AgentMainParams, AgentMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';

type Request<M extends keyof AgentMainParams> = (
  request: AgentMainParams[M],
) => Promise<ResultEnvelope<AgentMainResult[M]>>;
type Read<M extends keyof AgentMainParams> = () => Promise<ResultEnvelope<AgentMainResult[M]>>;

/** Chrome control in Aura: the settings section, and a conversation's shared tabs. */
export interface ChromeControlApiV1 {
  status: Read<'chrome.status'>;
  setEnabled: Request<'chrome.set_enabled'>;
  beginPairing: Read<'chrome.pair.begin'>;
  revoke: Read<'chrome.revoke'>;
  /** Stops everything Chrome does for Aura: every share ends. */
  stop: Read<'chrome.stop'>;
  /** The folder the user loads in Chrome, in the file manager. */
  openExtensionFolder: Read<'chrome.extension.open_folder'>;
  shares: Request<'chrome.shares'>;
  /** The user's 完成 for a tab the model handed them. */
  finishAssist: Request<'chrome.assist.finish'>;
}
