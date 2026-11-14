import type { AgentAttachmentDraft } from '@aura/contracts/generated';
import { MODEL_PAGE_ID } from '../../../shared/browser-api';
import { browserApi, browserStore } from '../browser/browser-store';
import { browserTabs } from '../browser/browser-tabs';
import { chromeControl } from '../chrome/bridge';
import { toolTabs } from '../right-workspace/tool-tabs';
import { addTerminal, terminalApi, terminalSnapshot, terminalText } from '../terminal/store';
import { notHandedOver, notReadPage } from './capture-failures';

/** The most of a terminal's lines a message keeps. */
export const CAPTURED_LINES = 2000;

/**
 * A terminal, a page of the built-in browser or a tab of the user's Chrome a message may refer to:
 * handed to the model as the message is sent, with what it shows then.
 */
export interface CaptureOption {
  kind: 'terminal' | 'page' | 'chrome';
  /** The terminal's id, the page's, or Chrome's own id for the tab. */
  id: string;
  /** As its tab is named. */
  title: string;
  /** A terminal's shell; a page's or tab's address. */
  detail: string;
  /** A terminal the model opened, which its terminal tools read further. */
  byModel?: boolean;
}

/**
 * The conversation's terminals, the user's and the model's, named as their tabs are: those still
 * running, and ended ones whose tab is still open.
 */
export function terminalOptions(session: string | undefined): CaptureOption[] {
  if (!session) return [];
  const tabs = toolTabs.get(session).tabs;
  return terminalSnapshot()
    .filter((record) => record.view.session_id === session)
    .flatMap(({ view }): CaptureOption[] => {
      const tab = tabs.find((open) => open.terminalId === view.terminal_id);
      // Ended with no tab showing it, it is gone for the user.
      if (view.end && tab === undefined) return [];
      const number = tab?.number;
      return [
        {
          kind: 'terminal',
          id: view.terminal_id,
          title:
            view.owner === 'agent'
              ? view.title || '模型终端'
              : number === undefined
                ? '终端'
                : `终端 ${number}`,
          detail: view.shell === 'cmd' ? 'CMD' : 'PowerShell 7',
          byModel: view.owner === 'agent',
        },
      ];
    });
}

/** The conversation's pages in the built-in browser, by title and address. */
export function pageOptions(session: string | undefined): CaptureOption[] {
  if (!session) return [];
  return browserTabs.get(session).pages.flatMap((page): CaptureOption[] => {
    const state = browserStore.get(session, page);
    if (!state || state.closed || !state.url) return [];
    return [
      {
        kind: 'page',
        id: page,
        title: state.title.trim() || (page === MODEL_PAGE_ID ? '模型的页面' : state.url),
        detail: state.url,
      },
    ];
  });
}

/** A file name for the capture: its tab's name, as Windows allows it. */
function fileName(title: string): string {
  const plain = [...title.replace(/[<>:"/\\|?*]/gu, '_')]
    .filter((char) => char >= ' ')
    .join('')
    .trim()
    .slice(0, 80)
    .replace(/[. ]+$/u, '');
  return `${plain || 'capture'}.txt`;
}

/** Why a capture could not be read, as the composer says it. */
export class CaptureUnavailable extends Error {}

/** What a terminal shows or a page holds now, as the message's attachment. */
export async function capture(
  session: string,
  option: CaptureOption,
): Promise<AgentAttachmentDraft> {
  if (option.kind === 'terminal') {
    // Handed over before the message goes: the model's first step finds it its own.
    const handed = await terminalApi()?.handOver({ session_id: session, terminal_id: option.id });
    if (!handed?.ok) throw new CaptureUnavailable(notHandedOver(option.title, handed?.error));
    await addTerminal(handed.value);
    const text = await terminalText(option.id, CAPTURED_LINES);
    if (text === undefined)
      throw new CaptureUnavailable(
        `读不到「${option.title}」的内容，消息没有发出，内容仍在输入框里；请先在侧栏打开这个终端再发。`,
      );
    // Named by its id, which the model's terminal tools take.
    const what = option.byModel
      ? `${option.detail}; your terminal \`${option.id}\`, which terminal.read reads further`
      : `${option.detail}; the user handed it to you as terminal \`${option.id}\`: terminal.read reads what it prints from now on and terminal.write types into it; it stays the user's, who can take it back at any time, and terminal.close is refused`;
    return {
      kind: 'capture',
      name: fileName(option.title),
      text: `Terminal "${option.title}" (${what}) as it was when this message was sent, up to its last 2,000 lines:\n\n${text}\n`,
    };
  }
  if (option.kind === 'chrome') return chromeCapture(session, option);
  const api = browserApi();
  let handedAs: string | undefined;
  if (option.id !== MODEL_PAGE_ID) {
    // Handed over before the message goes: the model's first step finds it by its id.
    const handed = await api?.command({ kind: 'hand-over', sessionId: session, pageId: option.id });
    handedAs = handed?.ok ? handed.state?.handedAs : undefined;
    if (handedAs === undefined)
      throw new CaptureUnavailable(
        notHandedOver(option.title, handed && !handed.ok ? handed.message : undefined),
      );
    if (handed?.ok && handed.state !== undefined) browserStore.put(handed.state);
  }
  const result = await api?.command({ kind: 'text', sessionId: session, pageId: option.id });
  if (!result?.ok || !result.text)
    throw new CaptureUnavailable(
      notReadPage(option.title, result && !result.ok ? result.message : undefined),
    );
  const { title, url, text, truncated } = result.text;
  const named = title.trim() || option.title;
  // Named by the id the model's browser tools take for it.
  const whose =
    handedAs === undefined
      ? 'your own page, which the browser tools act on without page'
      : `the user handed it to you as page \`${handedAs}\`: the browser tools act on it when given page "${handedAs}"; it stays the user's, who can take it back at any time, and browser.close does not close it`;
  return {
    kind: 'capture',
    name: fileName(named),
    text:
      `Page "${named}" (${url}; ${whose}) of the built-in browser as it was when this message was sent:\n\n${text}\n` +
      (truncated
        ? "\n[The page's text goes on; only its first 200,000 characters were kept.]\n"
        : ''),
  };
}

/** A Chrome tab: shared with the conversation before the message goes, and read as it is then. */
async function chromeCapture(
  session: string,
  option: CaptureOption,
): Promise<AgentAttachmentDraft> {
  const handed = await chromeControl()?.handOverTab({
    session_id: session,
    tab_id: Number(option.id),
  });
  if (!handed?.ok) throw new CaptureUnavailable(notHandedOver(option.title, handed?.error));
  const { tab, title, url, snapshot, snapshot_truncated: truncated } = handed.value;
  const named = title.trim() || option.title;
  // Named by the reference Chrome's tools take for it.
  return {
    kind: 'capture',
    name: fileName(named),
    text:
      `Chrome tab "${named}" (${url}; the user shared it with you as tab \`${tab}\`: the chrome tools act on it when given tab "${tab}"; it stays the user's, who can stop sharing it at any time, and chrome.close closes only tabs chrome.open opened) as it was when this message was sent:\n\n${snapshot}\n` +
      (truncated
        ? '\n[The tab goes on; chrome.snapshot with a larger max_chars reads more.]\n'
        : ''),
  };
}
