/** What the service worker tells the side panel, and what the side panel asks of it. */
import type { Assisting } from './assist.js';
import type { ConnectionState } from './native.js';
import type { SessionChoice, ShareScope, ShareView } from './protocol.js';

export const PANEL_PORT = 'aura-side-panel';

export interface PanelState {
  connection: ConnectionState;
  sessions: SessionChoice[];
  shares: ShareView[];
  assist: Assisting | null;
  /** Why Core refused the latest share, in the user's words. */
  share_refused: string | null;
}

export type PanelRequest =
  | { type: 'pair'; code: string }
  | {
      type: 'share';
      tab_id: number;
      window_id: number;
      url: string;
      title: string;
      session_id: string;
      scope: ShareScope;
    }
  | { type: 'unshare'; tab_id: number }
  | { type: 'stop' }
  | { type: 'answer_origin'; tab_id: number; origin: string; allow: boolean }
  | { type: 'assist'; outcome: 'done' | 'declined' }
  | { type: 'reconnect' };
