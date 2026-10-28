/**
 * Reading a shared tab: its accessibility snapshot with refs and what it reported since the last
 * result, a screenshot, waiting for text or time, and its console.
 */
import { BOXED, boxesOf, elementBox, renderSnapshot, type AxNode } from '../page/facade.js';
import {
  CommandError,
  type ConsoleLevel,
  type ConsoleReport,
  type PageReport,
  type ScreenshotReport,
} from '../protocol.js';
import { failed, refused, settle, stopIfCancelled, type CommandContext } from './context.js';

export const SNAPSHOT_CHARS = 12_000;
/** A screenshot larger than this, as base64, is refused: about 12 MB of PNG. */
const LARGEST_SCREENSHOT = 16 * 1024 * 1024;
const TALLEST_PAGE = 6_000;
/** How long a capture may wait for Chrome to draw the tab. */
const CAPTURE_LIMIT_MS = 10_000;
const BACKGROUND =
  '这个标签页在后台：Chrome 不绘制后台标签页和最小化窗口里的页面，截不了图。请用 chrome.snapshot 读取页面文字；需要看画面时，请用户把这个标签页切到前面后再截。';
const NOT_DRAWN =
  'Chrome 10 秒内没有画出这个标签页（窗口可能被挡住），截图没有完成。请用 chrome.snapshot 读取页面文字；需要看画面时，请用户把这个窗口切到前面后再截。';

/** Chrome draws only the tab in front of a window that is not minimized; a capture of any other
 * tab waits for a frame that never comes. */
async function drawn(context: CommandContext, tabId: number): Promise<boolean> {
  const tab = await context.tabs.get(tabId);
  if (!tab.active) return false;
  const window = await context.windows.get(tab.windowId);
  return window.state !== 'minimized';
}

/** `work`, or `chrome.not_drawn` once `ms` pass without it. */
async function within<T>(work: Promise<T>, ms: number): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new CommandError('chrome.not_drawn', NOT_DRAWN)), ms);
  });
  try {
    return await Promise.race([work, late]);
  } finally {
    clearTimeout(timer);
  }
}

export async function report(
  context: CommandContext,
  tabId: number,
  maxChars = SNAPSHOT_CHARS,
  notice: string | null = null,
): Promise<PageReport> {
  const page = await context.pages.page(tabId);
  const { nodes = [] } = await page.send<{ nodes?: AxNode[] }>('Accessibility.getFullAXTree');
  const boxed = nodes
    .filter((node) => !node.ignored && BOXED.has(String(node.role?.value ?? '')))
    .flatMap((node) => (node.backendDOMNodeId === undefined ? [] : [node.backendDOMNodeId]));
  const snapshot = renderSnapshot(nodes, page.refs, maxChars, await boxesOf(page, boxed));
  const tab = await context.tabs.get(tabId);
  return {
    url: tab.url ?? '',
    title: tab.title ?? '',
    snapshot: snapshot.text,
    snapshot_truncated: snapshot.truncated,
    console: page.takeNew(),
    dialogs: page.takeDialogs(),
    notice,
  };
}

/** Width and height from a PNG's header. */
export function pngSize(base64: string): { width: number; height: number } {
  const header = atob(base64.slice(0, 32));
  const at = (index: number) =>
    ((header.charCodeAt(index) << 24) |
      (header.charCodeAt(index + 1) << 16) |
      (header.charCodeAt(index + 2) << 8) |
      header.charCodeAt(index + 3)) >>>
    0;
  return { width: at(16), height: at(20) };
}

interface Metrics {
  cssVisualViewport?: {
    clientWidth?: number;
    clientHeight?: number;
    pageX?: number;
    pageY?: number;
  };
  cssContentSize?: { width?: number; height?: number };
}

export async function viewport(context: CommandContext, tabId: number) {
  const page = await context.pages.page(tabId);
  const metrics = await page.send<Metrics>('Page.getLayoutMetrics');
  const visual = metrics.cssVisualViewport ?? {};
  return {
    width: Math.max(1, Math.round(visual.clientWidth ?? 1280)),
    height: Math.max(1, Math.round(visual.clientHeight ?? 800)),
    x: visual.pageX ?? 0,
    y: visual.pageY ?? 0,
    contentWidth: Math.ceil(metrics.cssContentSize?.width ?? visual.clientWidth ?? 1280),
    contentHeight: Math.ceil(metrics.cssContentSize?.height ?? visual.clientHeight ?? 800),
  };
}

export async function screenshot(
  context: CommandContext,
  tabId: number,
  element: string | null,
  fullPage: boolean,
): Promise<ScreenshotReport> {
  if (!(await drawn(context, tabId))) throw new CommandError('chrome.background_tab', BACKGROUND);
  const page = await context.pages.page(tabId);
  const view = await viewport(context, tabId);
  let clip: { x: number; y: number; width: number; height: number };
  if (element) {
    const box = await elementBox(page, element);
    if (typeof box === 'string') throw refused(box);
    clip = {
      x: Math.max(0, Math.floor(box.x + view.x)),
      y: Math.max(0, Math.floor(box.y + view.y)),
      width: Math.max(1, Math.ceil(box.width)),
      height: Math.max(1, Math.ceil(box.height)),
    };
  } else if (fullPage) {
    clip = {
      x: 0,
      y: 0,
      width: view.contentWidth,
      height: Math.min(Math.max(view.height, view.contentHeight), TALLEST_PAGE),
    };
  } else {
    clip = { x: view.x, y: view.y, width: view.width, height: view.height };
  }
  const { data } = await within(
    page.send<{ data?: string }>('Page.captureScreenshot', {
      format: 'png',
      captureBeyondViewport: fullPage || element !== null,
      clip: { ...clip, scale: 1 },
    }),
    CAPTURE_LIMIT_MS,
  );
  if (!data) throw failed('截图为空：页面还没有画出内容，请稍后再试。');
  if (data.length > LARGEST_SCREENSHOT)
    throw refused('截图太大：请截取单个元素（ref），或不截整页。');
  const tab = await context.tabs.get(tabId);
  return { url: tab.url ?? '', title: tab.title ?? '', ...pngSize(data), png_base64: data };
}

async function pageText(context: CommandContext, tabId: number): Promise<string> {
  const page = await context.pages.page(tabId);
  const { result } = await page.send<{ result?: { value?: unknown } }>('Runtime.evaluate', {
    expression: 'document.body ? document.body.innerText : ""',
    returnByValue: true,
  });
  return typeof result?.value === 'string' ? result.value : '';
}

export async function wait(
  context: CommandContext,
  tabId: number,
  text: string | null,
  textGone: string | null,
  timeMs: number | null,
  timeoutMs: number,
): Promise<PageReport> {
  if (timeMs !== null) await context.sleep(timeMs);
  const end = context.now() + timeoutMs;
  const met = async () => {
    if (!text && !textGone) return true;
    const shown = await pageText(context, tabId);
    return (!text || shown.includes(text)) && (!textGone || !shown.includes(textGone));
  };
  let ok = await met();
  while (!ok && context.now() < end) {
    stopIfCancelled(context);
    await context.sleep(250);
    ok = await met();
  }
  await settle(context, tabId, 1_000);
  const what = text ? `出现「${text}」` : `「${textGone ?? ''}」消失`;
  const notice = ok ? null : `等了 ${Math.round(timeoutMs / 1000)} 秒，仍未${what}`;
  return report(context, tabId, SNAPSHOT_CHARS, notice);
}

export async function consoleOf(
  context: CommandContext,
  tabId: number,
  level: ConsoleLevel | null,
  maxEntries: number,
): Promise<ConsoleReport> {
  const page = await context.pages.page(tabId);
  const { entries, truncated } = page.all(level ?? 'all', maxEntries);
  const tab = await context.tabs.get(tabId);
  return { url: tab.url ?? '', entries, truncated };
}
