/**
 * Messages between Aura's Core and this extension, in the shapes `aura-chrome-domain` defines
 * (`protocol.rs` and `commands.rs`). `aura-chrome-host` relays them unchanged; the fixture test
 * reads the same JSON the Rust side writes, so the two cannot drift apart unnoticed.
 */
export const PROTOCOL_VERSION = 1;
/** The largest message Chrome accepts from the relay, in bytes. */
export const MAX_FROM_CORE_BYTES = 1024 * 1024;

export type ShareScope = 'session' | 'turn';
export type ShareState = 'active' | 'awaiting_origin' | 'with_user';
export type PairRefusal =
  | 'code_invalid'
  | 'code_expired'
  | 'too_many_attempts'
  | 'extension_not_allowed'
  | 'protocol_unsupported';
export type AuthRefusal = 'not_paired' | 'mac_invalid' | 'protocol_unsupported';
export type HistoryAction = 'back' | 'forward' | 'reload';
export type MouseButton = 'left' | 'right' | 'middle';
export type ScrollDirection = 'up' | 'down' | 'left' | 'right';
export type ConsoleLevel = 'error' | 'warning' | 'all';
export type Intent = 'click' | 'type' | 'press' | 'select' | 'hover' | 'scroll';
export type AssistOutcome = 'done' | 'declined' | 'timed_out' | 'closed';

export interface SessionChoice {
  session_id: string;
  title: string;
  workspace: string;
}

export interface ShareView {
  tab_id: number;
  session_id: string;
  session_title: string;
  origin: string;
  state: ShareState;
  scope: ShareScope;
}

export type CommandOutcome =
  { status: 'done'; value: unknown } | { status: 'failed'; code: string; message: string };

export type ExtensionMessage =
  | { type: 'hello'; extension_id: string; extension_version: string }
  | { type: 'pair'; code: string }
  | { type: 'authenticate'; mac: string }
  | { type: 'result'; id: number; outcome: CommandOutcome }
  | { type: 'pong'; id: number }
  | { type: 'list_sessions' }
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
  | { type: 'tab_changed'; tab_id: number; url: string; title: string }
  | { type: 'tab_closed'; tab_id: number }
  | { type: 'stop' }
  | { type: 'answer_origin'; tab_id: number; origin: string; allow: boolean };

export type CoreMessage =
  | { type: 'pairing_required' }
  | { type: 'paired'; secret: string }
  | { type: 'pair_refused'; reason: PairRefusal }
  | { type: 'challenge'; nonce: string; protocol: number }
  | { type: 'authenticated' }
  | { type: 'auth_refused'; reason: AuthRefusal }
  /** `origin` is the site Core authorized the command for; a tab elsewhere changes nothing. */
  | { type: 'command'; id: number; command: unknown; origin?: string }
  | { type: 'cancel'; id: number }
  | { type: 'ping'; id: number }
  | { type: 'revoked' }
  | { type: 'sessions'; sessions: SessionChoice[] }
  | { type: 'shares'; shares: ShareView[] }
  | { type: 'share_refused'; tab_id: number; reason: string }
  /** The user pressed 完成 in Aura for the tab the model handed them. */
  | { type: 'assist_done'; tab_id: number };

/** One command for one shared tab; `tab_id` is Chrome's own id, which Core resolved. */
export type ChromeCommand =
  | { op: 'open'; url: string }
  | { op: 'navigate'; tab_id: number; url: string | null; action: HistoryAction | null }
  | { op: 'snapshot'; tab_id: number; max_chars: number }
  | { op: 'screenshot'; tab_id: number; element: string | null; full_page: boolean }
  | {
      op: 'inspect';
      tab_id: number;
      element: string | null;
      x: number | null;
      y: number | null;
      intent: Intent;
    }
  | {
      op: 'click';
      tab_id: number;
      element: string | null;
      x: number | null;
      y: number | null;
      button: MouseButton;
      double: boolean;
    }
  | {
      op: 'type';
      tab_id: number;
      element: string | null;
      text: string;
      clear: boolean;
      submit: boolean;
    }
  | { op: 'press'; tab_id: number; key: string; repeat: number; hold_ms: number }
  | {
      op: 'scroll';
      tab_id: number;
      direction: ScrollDirection;
      element: string | null;
      amount: number;
    }
  | { op: 'hover'; tab_id: number; element: string }
  | { op: 'select'; tab_id: number; element: string; values: string[] }
  | {
      op: 'wait';
      tab_id: number;
      text: string | null;
      text_gone: string | null;
      time_ms: number | null;
      timeout_ms: number;
    }
  | { op: 'console'; tab_id: number; level: ConsoleLevel | null; max_entries: number }
  | { op: 'close'; tab_id: number }
  | { op: 'assist'; tab_id: number; reason: string; timeout_seconds: number }
  | { op: 'release'; tab_ids: number[] };

export interface ConsoleEntry {
  /** log, info, warning, error, exception or failed_load. */
  level: string;
  text: string;
  source: string | null;
}

/** What an action would touch, found before Core decides on it. */
export interface Inspection {
  refused: string | null;
  sensitive: boolean;
  origin: string;
}

export interface PageReport {
  url: string;
  title: string;
  snapshot: string;
  snapshot_truncated: boolean;
  console: ConsoleEntry[];
  dialogs: string[];
  notice: string | null;
}

export interface OpenedTab {
  tab_id: number;
  window_id: number;
  url: string;
  title: string;
}

export interface OpenReport {
  tab: OpenedTab;
  page: PageReport;
}

export interface ScreenshotReport {
  url: string;
  title: string;
  width: number;
  height: number;
  png_base64: string;
}

export interface ConsoleReport {
  url: string;
  entries: ConsoleEntry[];
  truncated: boolean;
}

export interface AssistReport {
  outcome: AssistOutcome;
  page: PageReport | null;
}

export interface CloseReport {
  closed: boolean;
}

/** A command that failed in the extension's own words; `chrome.refused…` changed nothing. */
export class CommandError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}
