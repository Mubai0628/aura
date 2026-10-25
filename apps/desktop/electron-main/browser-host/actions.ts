/**
 * Carrying out Core's command on a session's model page. Core authorized it; this checks
 * again that the page is still where Core saw it and that the user has not taken it over, acts
 * with Aura's own input, and answers with what the page looks like afterwards.
 */
import type {
  BrowserHostActionV1,
  BrowserHostCommandV1,
  BrowserHostResultV1,
  BrowserPageResult,
} from '@aura/contracts/generated';
import { MODEL_PAGE_ID } from '../../shared/browser-api.js';
import type { AssistOutcome } from './assist.js';
import type { PageDebugger } from './cdp.js';
import {
  boxesOf,
  elementBox,
  elementPoint,
  fieldAttributes,
  nodeOf,
  sensitiveField,
  type Point,
} from './elements.js';
import { click, hover, insertText, mark, press, wheel } from './input.js';
import { inspectElement, performanceCounters } from './inspection.js';
import { parseKey } from './keys.js';
import type { BrowserPages, PageContents, UserTouch } from './pages.js';
import { isInput, refused } from './protocol.js';
import { siteOfUrl } from './site-rules.js';
import { mediaSection, pageMedia } from './media.js';
import { BOXED, renderSnapshot, type AxNode, type Box } from './snapshot.js';

export const VIEWPORT = { width: 1280, height: 800 };
const SNAPSHOT_CHARS = 12_000;
const SCRIPT_MS = 10_000;
const VALUE_CHARS = 20_000;
const DEBUGGING_OFF = '网页调试没有开启：只有用户能在 设置 → 网络与浏览器 打开“允许模型调试网页”。';
/**
 * How the model learns of the user's takeover, and how to wait for it.
 */
const TAKEN_OVER =
  '用户已接管这个页面，Aura 的输入暂停，这一步没有执行。还要用这个页面时，调用 browser.request_user 请用户操作完后点“完成”（这一步没加载它时用 tools.call：{"tool": "browser.request_user", "arguments": {"reason": "…"}}）：用户点“完成”或“交还 Aura”时它立即返回现在的页面，不必等用户另发消息。';
const TOUCHED: Record<UserTouch, { refusal: string; notice: string }> = {
  took_over: {
    refusal:
      '用户刚才接管过这个页面，已经交还，页面可能被改过，这一步没有执行：先用 browser.snapshot 看现在的页面再操作。',
    notice: '用户接管过这个页面，已经交还；下面是交还后的样子。',
  },
  navigated: {
    refusal:
      '用户刚才自己在这个页面上打开了网址、前进后退或刷新，页面可能变了，这一步没有执行：先用 browser.snapshot 看现在的页面再操作。',
    notice: '用户刚才自己在这个页面上打开了网址、前进后退或刷新；下面是现在的样子。',
  },
};
const HELD_NOTICE =
  '用户正在操作这个页面（已接管），下面是现在的样子；Aura 的输入暂停，可以用 browser.request_user 等用户操作完。';
/** What shows the model its page as it is now. */
const LOOKS = new Set(['open', 'navigate', 'snapshot', 'screenshot', 'request_user']);

interface Picture {
  getSize(): { width: number; height: number };
  resize(options: { width?: number; height?: number }): Picture;
  toJPEG(quality: number): Buffer;
  isEmpty(): boolean;
}
/** A page's web contents, with the capture Electron gives every page. */
export type CapturablePage = PageContents & {
  capturePage(rect?: { x: number; y: number; width: number; height: number }): Promise<Picture>;
};

export interface ExecutorDeps {
  pages: BrowserPages;
  /** The page's DevTools connection, made on first use. */
  debuggerFor(contents: PageContents): PageDebugger;
  /** Sets the page's fixed viewport, drawn at the sidebar's width; returns the scale. */
  emulate(sessionId: string, devtools: PageDebugger): Promise<number>;
  /** Shows the model's request for the user's help and waits for how it ends. */
  assist(
    sessionId: string,
    commandId: string,
    reason: string,
    timeoutMs: number,
  ): Promise<AssistOutcome | 'withdrawn'>;
}

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));
const hostOf = (url: string) => siteOfUrl(url)?.host ?? null;

/** Waits for the page to stop loading, at most `limit`; a short quiet moment either way. */
async function settle(contents: PageContents, limit: number): Promise<boolean> {
  const end = Date.now() + limit;
  await sleep(120);
  while (contents.isLoading() && Date.now() < end) await sleep(100);
  await sleep(150);
  return !contents.isLoading();
}

async function pageResult(
  contents: PageContents,
  devtools: PageDebugger,
  maxChars: number,
  notice: string | null,
): Promise<BrowserPageResult> {
  const { nodes = [] } = await devtools.send<{ nodes?: AxNode[] }>('Accessibility.getFullAXTree');
  const boxed = nodes
    .filter((node) => !node.ignored && BOXED.has(String(node.role?.value ?? '')))
    .flatMap((node) => (node.backendDOMNodeId === undefined ? [] : [node.backendDOMNodeId]));
  const snapshot = renderSnapshot(nodes, devtools.refs, maxChars, await boxesOf(devtools, boxed));
  return {
    url: contents.getURL(),
    title: contents.getTitle(),
    // The page's videos, sounds and larger pictures, with the addresses to download them.
    snapshot: snapshot.text + mediaSection(await pageMedia(devtools, contents.getURL())),
    snapshot_truncated: snapshot.truncated,
    console: devtools.takeNew(),
    dialogs: devtools.takeDialogs(),
    notice,
  };
}

const done = (
  command: BrowserHostCommandV1,
  url: string,
  result: unknown,
): BrowserHostResultV1 => ({
  command_id: command.command_id,
  page_url: url,
  outcome: { outcome: 'completed', result: result as Record<string, never>, image: null },
});

/**
 * A screenshot in page pixels. DevTools draws it at the page's own size whatever the sidebar's
 * scale; while the window is minimized it never answers, so the page is drawn once at full size
 * for Electron's capture instead (nobody is looking at it then).
 */
async function capture(
  contents: CapturablePage,
  devtools: PageDebugger,
  sessionId: string,
  deps: ExecutorDeps,
  element: string | null | undefined,
  fullPage: boolean,
): Promise<{ jpeg: Buffer; width: number; height: number } | string> {
  let clip: Box;
  if (element) {
    const box = await elementBox(devtools, element);
    if (typeof box === 'string') return box;
    const { x: left, y: top } = await scrollOffset(devtools);
    clip = { ...roundBox(box), x: Math.floor(box.x + left), y: Math.floor(box.y + top) };
  } else if (fullPage) {
    const metrics = await devtools.send<{ cssContentSize?: { height?: number } }>(
      'Page.getLayoutMetrics',
    );
    const height = Math.ceil(metrics.cssContentSize?.height ?? 0);
    clip = {
      x: 0,
      y: 0,
      width: VIEWPORT.width,
      height: Math.min(Math.max(VIEWPORT.height, height), 6_000),
    };
  } else {
    const { x, y } = await scrollOffset(devtools);
    clip = { x, y, ...VIEWPORT };
  }
  const drawn = await Promise.race([
    devtools
      .send<{ data?: string }>('Page.captureScreenshot', {
        format: 'jpeg',
        quality: 80,
        captureBeyondViewport: fullPage || Boolean(element),
        clip: { ...clip, scale: 1 },
      })
      .then((result) => result.data),
    sleep(4_000).then(() => undefined),
  ]).catch(() => undefined);
  if (drawn !== undefined) {
    const jpeg = Buffer.from(drawn, 'base64');
    return { jpeg, width: Math.round(clip.width), height: Math.round(clip.height) };
  }
  // Minimized: draw the page at full size for one capture, then put the sidebar's scale back.
  await devtools.send('Emulation.setDeviceMetricsOverride', {
    ...VIEWPORT,
    height: fullPage ? clip.height : VIEWPORT.height,
    deviceScaleFactor: 1,
    mobile: false,
    scale: 1,
  });
  await sleep(250);
  let picture: Picture;
  try {
    const { x, y } = await scrollOffset(devtools);
    picture = element
      ? await contents.capturePage({
          ...roundBox(clip),
          x: Math.max(0, clip.x - x),
          y: Math.max(0, clip.y - y),
        })
      : await contents.capturePage();
  } finally {
    await deps.emulate(sessionId, devtools);
  }
  if (picture.isEmpty()) return '页面还没有画出任何内容，截图为空；请等页面加载后再试。';
  if (picture.getSize().width > VIEWPORT.width) picture = picture.resize({ width: VIEWPORT.width });
  const { width, height } = picture.getSize();
  return { jpeg: picture.toJPEG(80), width, height };
}

/** How far the page is scrolled, in page pixels. */
async function scrollOffset(devtools: PageDebugger): Promise<{ x: number; y: number }> {
  const { result } = await devtools.send<{ result?: { value?: { x?: number; y?: number } } }>(
    'Runtime.evaluate',
    { expression: '({ x: scrollX, y: scrollY })', returnByValue: true },
  );
  return { x: result?.value?.x ?? 0, y: result?.value?.y ?? 0 };
}

function roundBox(box: Box): { x: number; y: number; width: number; height: number } {
  return {
    x: Math.max(0, Math.floor(box.x)),
    y: Math.max(0, Math.floor(box.y)),
    width: Math.max(1, Math.ceil(box.width)),
    height: Math.max(1, Math.ceil(box.height)),
  };
}

interface Evaluated {
  result?: { type?: string; value?: unknown; unserializableValue?: string; description?: string };
  exceptionDetails?: { text?: string; exception?: { description?: string } };
}

/** A model's script in the page: its value as JSON text, or what it threw. */
async function evaluate(
  devtools: PageDebugger,
  expression: string,
): Promise<{ value: string | null; value_truncated: boolean; exception: string | null }> {
  const run = (byValue: boolean) =>
    devtools.send<Evaluated>('Runtime.evaluate', {
      expression,
      awaitPromise: true,
      returnByValue: byValue,
      userGesture: false,
      timeout: SCRIPT_MS,
    });
  const timedOut = sleep(SCRIPT_MS + 500).then(() => 'timed out' as const);
  let answer = await Promise.race([run(true).catch((error: unknown) => error), timedOut]);
  if (answer === 'timed out')
    return {
      value: null,
      value_truncated: false,
      exception: `脚本 ${SCRIPT_MS / 1000} 秒内没有结束`,
    };
  // A value JSON cannot carry (a DOM node, a cycle) comes back as its description instead.
  if (answer instanceof Error) answer = await run(false);
  const { result, exceptionDetails } = answer as Evaluated;
  if (exceptionDetails !== undefined)
    return {
      value: null,
      value_truncated: false,
      exception: String(
        exceptionDetails.exception?.description ?? exceptionDetails.text ?? 'Uncaught error',
      ).slice(0, VALUE_CHARS),
    };
  const text =
    result?.type === 'undefined'
      ? 'undefined'
      : (result?.unserializableValue ??
        ('value' in (result ?? {}) ? JSON.stringify(result?.value) : result?.description) ??
        'undefined');
  return {
    value: text.slice(0, VALUE_CHARS),
    value_truncated: text.length > VALUE_CHARS,
    exception: null,
  };
}

async function textOnPage(devtools: PageDebugger): Promise<string> {
  const { result } = await devtools.send<{ result?: { value?: unknown } }>('Runtime.evaluate', {
    expression: 'document.body ? document.body.innerText : ""',
    returnByValue: true,
  });
  return typeof result?.value === 'string' ? result.value : '';
}

async function act(
  deps: ExecutorDeps,
  command: BrowserHostCommandV1,
  contents: CapturablePage,
  devtools: PageDebugger,
  scale: number,
): Promise<BrowserHostResultV1> {
  const { pages } = deps;
  const session = command.session_id;
  const action: BrowserHostActionV1 = command.action;
  const url = () => contents.getURL();
  const touched = pages.userHad(session);
  const held = pages.state(session, MODEL_PAGE_ID)?.userInControl
    ? HELD_NOTICE
    : touched === undefined
      ? null
      : TOUCHED[touched].notice;
  const page = (notice: string | null = null, chars = SNAPSHOT_CHARS) =>
    pageResult(contents, devtools, chars, notice ?? held).then((result) =>
      done(command, url(), result),
    );
  const no = (message: string) => refused(command, url(), message);
  const target = async (
    ref: string | null | undefined,
    fallback: Point,
  ): Promise<Point | string> => (ref ? elementPoint(devtools, ref) : fallback);
  switch (action.kind) {
    case 'open': {
      let failure: string | null = null;
      try {
        await pages.loadForModel(session, MODEL_PAGE_ID, action.url);
      } catch (error) {
        const why = pages.refusal(session, MODEL_PAGE_ID, action.url);
        if (why !== undefined) return no(why);
        failure = `打不开 ${action.url}：${error instanceof Error ? error.message : String(error)}`;
      }
      const loaded = await settle(contents, 20_000);
      return page(failure ?? (loaded ? null : '页面仍在加载，下面是当前的样子'));
    }
    case 'navigate': {
      const history = contents.navigationHistory;
      if (action.action === 'back' && !history.canGoBack()) return page('没有可以后退的页面');
      if (action.action === 'forward' && !history.canGoForward()) return page('没有可以前进的页面');
      pages.history(session, MODEL_PAGE_ID, action.action, true);
      const loaded = await settle(contents, 20_000);
      return page(loaded ? null : '页面仍在加载，下面是当前的样子');
    }
    case 'snapshot':
      return page(null, action.max_chars);
    case 'screenshot': {
      const shot = await capture(
        contents,
        devtools,
        session,
        deps,
        action.element,
        action.full_page,
      );
      if (typeof shot === 'string') return no(shot);
      return {
        command_id: command.command_id,
        page_url: url(),
        outcome: {
          outcome: 'completed',
          result: { url: url(), title: contents.getTitle() },
          image: {
            media_type: 'image/jpeg',
            width: shot.width,
            height: shot.height,
            data_base64: shot.jpeg.toString('base64'),
          },
        },
      };
    }
    case 'click': {
      const point = await target(action.element, { x: action.x ?? 0, y: action.y ?? 0 });
      if (typeof point === 'string') return no(point);
      mark(devtools, point);
      await click(devtools, point, scale, action.button, action.double);
      await settle(contents, 10_000);
      return page();
    }
    case 'type': {
      const attributes = await fieldAttributes(devtools, action.element);
      if (typeof attributes === 'string') return no(attributes);
      if (sensitiveField(attributes))
        return no('这是密码、支付卡或验证码这类敏感字段，Aura 不替模型输入；请用户自己填写。');
      if (action.element) {
        const node = nodeOf(devtools, action.element);
        if (typeof node === 'string') return no(node);
        await devtools.send('DOM.focus', { backendNodeId: node });
      }
      if (action.clear)
        await devtools.send('Runtime.evaluate', {
          expression:
            '(() => { const e = document.activeElement; if (e && "value" in e) { e.value = ""; e.dispatchEvent(new Event("input", { bubbles: true })); } else if (e && e.isContentEditable) { e.textContent = ""; } })()',
        });
      await insertText(devtools, action.text);
      if (action.submit) {
        const enter = parseKey('Enter');
        if (typeof enter !== 'string') await press(devtools, enter, 0);
      }
      await settle(contents, action.submit ? 10_000 : 1_000);
      return page();
    }
    case 'press': {
      const stroke = parseKey(action.key);
      if (typeof stroke === 'string') return no(stroke);
      for (let time = 0; time < Math.max(1, action.repeat); time += 1)
        await press(devtools, stroke, action.hold_ms);
      await settle(contents, 5_000);
      return page();
    }
    case 'scroll': {
      const point = await target(action.element, { x: VIEWPORT.width / 2, y: VIEWPORT.height / 2 });
      if (typeof point === 'string') return no(point);
      const [dx, dy] = { up: [0, -1], down: [0, 1], left: [-1, 0], right: [1, 0] }[
        action.direction
      ];
      await wheel(devtools, point, scale, (dx ?? 0) * action.amount, (dy ?? 0) * action.amount);
      await settle(contents, 1_000);
      return page();
    }
    case 'hover': {
      const point = await elementPoint(devtools, action.element);
      if (typeof point === 'string') return no(point);
      await hover(devtools, point, scale);
      await settle(contents, 1_000);
      return page();
    }
    case 'select': {
      const node = nodeOf(devtools, action.element);
      if (typeof node === 'string') return no(node);
      const { object } = await devtools.send<{ object?: { objectId?: string } }>(
        'DOM.resolveNode',
        { backendNodeId: node },
      );
      if (object?.objectId === undefined)
        return no(`元素引用 ${action.element} 已失效；请重新 browser.snapshot。`);
      const { result } = await devtools.send<{ result?: { value?: unknown } }>(
        'Runtime.callFunctionOn',
        {
          objectId: object.objectId,
          functionDeclaration:
            'function (values) { if (!(this instanceof HTMLSelectElement)) return -1; let matched = 0; for (const option of this.options) { const hit = values.includes(option.value) || values.includes(option.label) || values.includes(option.text.trim()); if (this.multiple) option.selected = hit; else if (hit && matched === 0) this.value = option.value; if (hit) matched += 1; } if (matched) { this.dispatchEvent(new Event("input", { bubbles: true })); this.dispatchEvent(new Event("change", { bubbles: true })); } return matched; }',
          arguments: [{ value: action.values }],
          returnByValue: true,
        },
      );
      if (result?.value === -1) return no(`${action.element} 不是下拉选择框（select）`);
      if (result?.value === 0) return no(`下拉框里没有这些选项：${action.values.join('、')}`);
      await settle(contents, 1_000);
      return page();
    }
    case 'wait': {
      if (action.time_ms !== undefined && action.time_ms !== null) await sleep(action.time_ms);
      const end = Date.now() + action.timeout_ms;
      const met = async () => {
        if (!action.text && !action.text_gone) return true;
        const text = await textOnPage(devtools);
        return (
          (!action.text || text.includes(action.text)) &&
          (!action.text_gone || !text.includes(action.text_gone))
        );
      };
      let ok = await met();
      while (!ok && Date.now() < end) {
        await sleep(250);
        ok = await met();
      }
      const what = action.text ? `出现「${action.text}」` : `「${action.text_gone ?? ''}」消失`;
      return page(ok ? null : `等了 ${Math.round(action.timeout_ms / 1000)} 秒，仍未${what}`);
    }
    case 'console': {
      const { entries, truncated } = devtools.all(action.level, action.max_entries);
      return done(command, url(), { url: url(), entries, truncated });
    }
    case 'close':
      return done(command, 'about:blank', { closed: true });
    case 'request_user': {
      const outcome = await deps.assist(
        session,
        command.command_id,
        action.reason,
        action.timeout_ms,
      );
      // Core no longer waits for an answer.
      if (outcome === 'withdrawn') return no('已停止等待用户');
      // The user may have changed anything: refs from before name nothing now.
      devtools.refs.clear();
      const state = pages.state(session, MODEL_PAGE_ID);
      if (outcome === 'closed' || state === undefined || state.closed || contents.isDestroyed())
        return done(command, 'about:blank', { outcome: 'closed', page: null });
      await settle(contents, 5_000);
      return done(command, url(), {
        outcome,
        page: await pageResult(contents, devtools, SNAPSHOT_CHARS, null),
      });
    }
    case 'evaluate': {
      if (pages.policy.debugging !== true) return no(DEBUGGING_OFF);
      const evaluated = await evaluate(devtools, action.expression);
      await settle(contents, 1_000);
      return done(command, url(), { url: url(), ...evaluated });
    }
    case 'network': {
      if (pages.policy.debugging !== true) return no(DEBUGGING_OFF);
      const { entries, truncated } = devtools.network.entries(
        action.failed_only,
        action.max_entries,
      );
      return done(command, url(), { url: url(), entries, truncated });
    }
    case 'inspect': {
      if (pages.policy.debugging !== true) return no(DEBUGGING_OFF);
      const inspected = await inspectElement(devtools, action.element);
      if (typeof inspected === 'string') return no(inspected);
      return done(command, url(), { url: url(), ...inspected });
    }
    case 'performance': {
      if (pages.policy.debugging !== true) return no(DEBUGGING_OFF);
      return done(command, url(), { url: url(), metrics: await performanceCounters(devtools) });
    }
  }
}

/** Carries out one command from Core on the session's model page. */
export async function execute(
  deps: ExecutorDeps,
  command: BrowserHostCommandV1,
): Promise<BrowserHostResultV1> {
  const { pages } = deps;
  const session = command.session_id;
  const action = command.action;
  const state = pages.state(session, MODEL_PAGE_ID);
  const open = state !== undefined && !state.closed;
  const current = () => pages.contents(session, MODEL_PAGE_ID)?.getURL() ?? 'about:blank';
  if (!pages.policy.enabled)
    return refused(command, current(), '内置浏览器已在设置中关闭（设置 → 网络与浏览器）。');
  if (action.kind === 'close') {
    pages.close(session, MODEL_PAGE_ID);
    return done(command, 'about:blank', { closed: open });
  }
  if (action.kind !== 'open' && !open)
    return refused(command, 'about:blank', '还没有打开页面：先用 browser.open 打开一个页面。');
  const access = {
    kind: 'model' as const,
    readOnly: command.access.read_only,
    approvedHosts: command.access.approved_hosts,
    fileRoot: command.access.file_root ?? undefined,
  };
  if (!open)
    pages.open(session, MODEL_PAGE_ID, access, action.kind === 'open' ? action.url : undefined);
  pages.setAccess(session, MODEL_PAGE_ID, access);
  const contents = pages.contents(session, MODEL_PAGE_ID) as CapturablePage | undefined;
  if (contents === undefined)
    return refused(command, 'about:blank', '页面没有打开，请重试 browser.open。');
  if (isInput(action)) {
    if (pages.state(session, MODEL_PAGE_ID)?.userInControl)
      return refused(command, current(), TAKEN_OVER);
    const touched = pages.userHad(session);
    if (touched !== undefined) return refused(command, current(), TOUCHED[touched].refusal);
    const now = hostOf(contents.getURL());
    const expected = command.expected_host ?? null;
    if (now !== expected)
      return refused(
        command,
        current(),
        `页面已经不在批准这一步时的${expected ?? '空白或本地页面'}（现在是 ${now ?? '空白或本地页面'}），没有执行；请先 browser.snapshot 查看。`,
      );
  }
  // While the model waits for the user's help, the page is the user's, not Aura's at work.
  const working = action.kind !== 'request_user';
  if (working) pages.setAgentActive(session, MODEL_PAGE_ID, true);
  try {
    await settle(contents, 5_000);
    const devtools = deps.debuggerFor(contents);
    await devtools.ready();
    const scale = await deps.emulate(session, devtools);
    const result = await act(deps, command, contents, devtools, scale);
    if (result.outcome.outcome === 'completed' && LOOKS.has(action.kind))
      pages.modelLooked(session);
    return result;
  } catch (error) {
    return {
      command_id: command.command_id,
      page_url: current(),
      outcome: {
        outcome: 'failed',
        message: error instanceof Error ? error.message : String(error),
        may_have_acted: isInput(action) || action.kind === 'open' || action.kind === 'navigate',
      },
    };
  } finally {
    if (working) pages.setAgentActive(session, MODEL_PAGE_ID, false);
  }
}
