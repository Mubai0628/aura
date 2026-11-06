import type { AgentMainParams, AgentMainResult } from './generated/routes.js';
import type { ResultEnvelope } from './aura-api.js';

type Request<M extends keyof AgentMainParams> = (
  request: AgentMainParams[M],
) => Promise<ResultEnvelope<AgentMainResult[M]>>;
type Read<M extends keyof AgentMainParams> = () => Promise<ResultEnvelope<AgentMainResult[M]>>;

/** Computer control in Aura: its settings, the programs for 「@」 and their icons, and a
 * conversation's 正在操控. */
export interface ComputerControlApiV1 {
  settings: Read<'computer.settings.get'>;
  updateSettings: Request<'computer.settings.update'>;
  /** The programs on the computer; none while computer control is off. */
  catalog: Request<'computer.catalog'>;
  /** The icons of programs of the catalog; none while computer control is off. */
  icons: Request<'computer.icons'>;
  /** The programs a conversation is controlling now. */
  controlling: Request<'computer.controlling'>;
  /** Stops one program for a conversation, or each program it is controlling now. */
  stop: Request<'computer.stop'>;
  /** Gives the desktop the user took over back to the conversation. */
  giveBack: Request<'computer.give_back'>;
  /** The approval card's 总是允许: the program goes on the settings' list. */
  alwaysAllow: Request<'computer.always_allow'>;
}
