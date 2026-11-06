import type { AgentAttachmentDraft } from '@aura/contracts/generated';
import { MODEL_PAGE_ID } from '../../../shared/browser-api';
import { browserApi, browserStore } from '../browser/browser-store';
import { browserTabs } from '../browser/browser-tabs';
import { toolTabs } from '../right-workspace/tool-tabs';
import { terminalSnapshot, terminalText } from '../terminal/store';

/** The most of a terminal's lines a message keeps. */
export const CAPTURED_LINES = 2000;

/** A terminal or a page of the built-in browser a message may refer to: sent as what it shows. */
export interface CaptureOption {
  kind: 'terminal' | 'page';
  /** The terminal's id, or the page's. */
  id: string;
  /** As its tab is named. */
  title: string;
  /** A terminal's shell; a page's address. */
  detail: string;
  /** A terminal the model opened, which its terminal tools read further. */
  byModel?: boolean;
}

/** The conversation's terminals, the user's and the model's, named as their tabs are. */
export function terminalOptions(session: string | undefined): CaptureOption[] {
  if (!session) return [];
  const tabs = toolTabs.get(session).tabs;
  return terminalSnapshot()
    .filter((record) => record.view.session_id === session)
    .map(({ view }) => {
      const number = tabs.find((tab) => tab.terminalId === view.terminal_id)?.number;
      return {
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
      };
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
    const text = await terminalText(option.id, CAPTURED_LINES);
    if (text === undefined)
      throw new CaptureUnavailable(`读不到「${option.title}」的内容，请先在侧栏打开这个终端。`);
    // The model's own terminal is named by its id, which its terminal tools take.
    const what = option.byModel
      ? `${option.detail}; your terminal \`${option.id}\`, which terminal.read reads further`
      : option.detail;
    return {
      kind: 'capture',
      name: fileName(option.title),
      text: `Terminal "${option.title}" (${what}) as it was when this message was sent, up to its last 2,000 lines:\n\n${text}\n`,
    };
  }
  const result = await browserApi()?.command({
    kind: 'text',
    sessionId: session,
    pageId: option.id,
  });
  if (!result?.ok || !result.text)
    throw new CaptureUnavailable(
      result && !result.ok ? result.message : `读不到「${option.title}」的内容。`,
    );
  const { title, url, text, truncated } = result.text;
  const named = title.trim() || option.title;
  return {
    kind: 'capture',
    name: fileName(named),
    text:
      `Page "${named}" (${url}) of the built-in browser as it was when this message was sent:\n\n${text}\n` +
      (truncated
        ? "\n[The page's text goes on; only its first 200,000 characters were kept.]\n"
        : ''),
  };
}
