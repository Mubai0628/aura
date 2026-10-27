/**
 * What an action would touch, looked at before Core decides on it and again as the action runs.
 * Typing into a field that holds a secret, anything in a CAPTCHA and opening a file chooser are
 * the user's alone; an action that buys, pays, deletes, sends, publishes or transfers - or works
 * in a form that takes payment details - is asked about every time.
 */
import type { Intent, Inspection } from '../protocol.js';
import { nodeOf, sensitiveField, type PageDebugger, type Point } from './facade.js';

export interface Target {
  tag: string;
  attributes: Record<string, string>;
  /** The element's own words: its text, value, label and title. */
  text: string;
  /** It is in a form with card, security-code or one-time-code fields. */
  paymentForm: boolean;
  captcha: boolean;
  fileChooser: boolean;
}

const SENSITIVE =
  /\b(?:buy|purchase|order|checkout|check out|pay|payment|delete|remove|send|publish|post|transfer|withdraw|donate|subscribe)\b|购买|下单|结算|支付|付款|删除|移除|发送|发布|转账|汇款|提现|捐赠|订阅/iu;

/** Run on the element in the page; returns a `Target`. */
const DESCRIBE = `function () {
  const element = this;
  const attributes = {};
  for (const name of element.getAttributeNames())
    attributes[name.toLowerCase()] = (element.getAttribute(name) || '').slice(0, 200);
  const words = [element.innerText, element.value, element.getAttribute('aria-label'),
    element.getAttribute('title'), element.getAttribute('alt')]
    .filter((value) => typeof value === 'string' && value.trim() !== '')
    .join(' ').replace(/\\s+/g, ' ').slice(0, 300);
  const form = element.form || element.closest('form');
  const paymentForm = Boolean(form) && Array.from(form.querySelectorAll('input')).some((field) =>
    /(^|\\s)cc-/.test(field.autocomplete || '') || /cvv|cvc|card.?num|卡号/i.test(field.name || field.id || ''));
  const captcha = Boolean(element.closest('[class*="captcha" i],[id*="captcha" i]')) ||
    (element.tagName === 'IFRAME' && /captcha|recaptcha|hcaptcha|turnstile|arkoselabs/i.test(element.src || ''));
  const control = element.tagName === 'LABEL' ? element.control : element;
  const fileChooser = Boolean(control) && control.tagName === 'INPUT' && control.type === 'file';
  return { tag: element.tagName.toLowerCase(), attributes, text: words, paymentForm, captcha, fileChooser };
}`;

/** Why the user must do this themselves, or null. */
export function refusal(target: Target, intent: Intent): string | null {
  if (intent === 'hover' || intent === 'scroll') return null;
  if (target.captcha) return '这是验证码（CAPTCHA）';
  if (intent === 'click' && target.fileChooser) return '这会打开选择文件的窗口';
  if (
    (intent === 'type' || intent === 'press' || intent === 'select') &&
    sensitiveField(target.attributes)
  )
    return '这是密码、验证码或支付卡这类敏感字段';
  return null;
}

/** Whether the user approves this action every time. */
export function sensitive(target: Target, intent: Intent): boolean {
  if (intent === 'hover' || intent === 'scroll') return false;
  if (target.paymentForm) return true;
  return intent === 'click' && SENSITIVE.test(`${target.text} ${target.attributes.name ?? ''}`);
}

/** `scheme://host[:port]` of an address, lowercase; the address itself when it has no host. */
export function originOf(url: string): string {
  const lower = url.trim().toLowerCase();
  const at = lower.indexOf('://');
  if (at < 0) return lower;
  const authority = lower.slice(at + 3).split(/[/?#]/u)[0] ?? '';
  const host = authority.slice(authority.lastIndexOf('@') + 1);
  return `${lower.slice(0, at)}://${host}`;
}

type Called = { result?: { value?: unknown }; exceptionDetails?: unknown };

/** What an action names: an element by its ref, a point of the viewport, or else the focus. */
export interface Where {
  ref: string | null;
  point: Point | null;
}

async function nodeAt(page: PageDebugger, point: Point): Promise<number | string> {
  try {
    const { backendNodeId } = await page.send<{ backendNodeId?: number }>(
      'DOM.getNodeForLocation',
      { x: Math.round(point.x), y: Math.round(point.y), includeUserAgentShadowDOM: false },
    );
    return backendNodeId ?? `坐标 (${point.x}, ${point.y}) 处没有元素`;
  } catch {
    return `坐标 (${point.x}, ${point.y}) 不在页面上`;
  }
}

/** The element an action names; a string says what is wrong. */
export async function describe(page: PageDebugger, where: Where): Promise<Target | string> {
  let called: Called;
  if (where.ref !== null || where.point !== null) {
    const node =
      where.ref !== null ? nodeOf(page, where.ref) : await nodeAt(page, where.point as Point);
    if (typeof node === 'string') return node;
    const { object } = await page
      .send<{ object?: { objectId?: string } }>('DOM.resolveNode', { backendNodeId: node })
      .catch(() => ({ object: undefined }));
    if (object?.objectId === undefined) return `元素已不在页面上；请重新 ${page.snapshotTool}。`;
    called = await page.send<Called>('Runtime.callFunctionOn', {
      objectId: object.objectId,
      functionDeclaration: DESCRIBE,
      returnByValue: true,
    });
  } else {
    called = await page.send<Called>('Runtime.evaluate', {
      expression: `(() => { const e = document.activeElement; return e && e !== document.body ? (${DESCRIBE}).call(e) : null; })()`,
      returnByValue: true,
    });
  }
  const value = called.result?.value;
  if (called.exceptionDetails !== undefined || typeof value !== 'object' || value === null)
    return '没有可以查看的元素';
  return value as Target;
}

/** The inspection Core asks for before it decides on an action. */
export async function inspect(
  page: PageDebugger,
  where: Where,
  intent: Intent,
  url: string,
): Promise<Inspection> {
  const origin = originOf(url);
  const target = await describe(page, where);
  // What names nothing is refused when the action runs, in the action's own words.
  if (typeof target === 'string') return { refused: null, sensitive: false, origin };
  return { refused: refusal(target, intent), sensitive: sensitive(target, intent), origin };
}
