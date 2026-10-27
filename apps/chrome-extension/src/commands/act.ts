/**
 * Acting in a shared tab with Aura's own input through DevTools: the user's mouse and keyboard
 * are never used. What only the user may do is checked again right before acting, because the
 * page can change after Core decided.
 */
import {
  click as clickAt,
  elementPoint,
  hover as hoverAt,
  insertText,
  mark,
  nodeOf,
  parseKey,
  press as pressKey,
  wheel,
  type PageDebugger,
  type Point,
} from '../page/facade.js';
import { describe, refusal, type Where } from '../page/inspect.js';
import type { ChromeCommand, Intent, PageReport } from '../protocol.js';
import { refused, settle, stopIfCancelled, type CommandContext } from './context.js';
import { report, viewport } from './look.js';

const USERS_OWN = '这一步没有执行；请用 chrome.request_user 请用户自己操作。';
const CLEAR =
  '(() => { const e = document.activeElement; if (e && "value" in e) { e.value = ""; e.dispatchEvent(new Event("input", { bubbles: true })); } else if (e && e.isContentEditable) { e.textContent = ""; } })()';
const SELECT =
  'function (values) { if (!(this instanceof HTMLSelectElement)) return -1; let matched = 0; for (const option of this.options) { const hit = values.includes(option.value) || values.includes(option.label) || values.includes(option.text.trim()); if (this.multiple) option.selected = hit; else if (hit && matched === 0) this.value = option.value; if (hit) matched += 1; } if (matched) { this.dispatchEvent(new Event("input", { bubbles: true })); this.dispatchEvent(new Event("change", { bubbles: true })); } return matched; }';

async function guard(page: PageDebugger, where: Where, intent: Intent): Promise<void> {
  const target = await describe(page, where);
  if (typeof target === 'string') return;
  const why = refusal(target, intent);
  if (why !== null) throw refused(`${why}，${USERS_OWN}`);
}

async function pointOf(page: PageDebugger, ref: string): Promise<Point> {
  const point = await elementPoint(page, ref);
  if (typeof point === 'string') throw refused(point);
  return point;
}

type Input = Extract<
  ChromeCommand,
  { op: 'click' | 'type' | 'press' | 'scroll' | 'hover' | 'select' }
>;

export async function act(context: CommandContext, command: Input): Promise<PageReport> {
  const tabId = command.tab_id;
  const page = await context.pages.page(tabId);
  stopIfCancelled(context);
  switch (command.op) {
    case 'click': {
      const point =
        command.element !== null
          ? await pointOf(page, command.element)
          : { x: command.x ?? 0, y: command.y ?? 0 };
      await guard(page, { ref: command.element, point }, 'click');
      mark(page, point);
      await clickAt(page, point, 1, command.button, command.double);
      await settle(context, tabId, 10_000);
      return report(context, tabId);
    }
    case 'type': {
      await guard(page, { ref: command.element, point: null }, 'type');
      if (command.element !== null) {
        const node = nodeOf(page, command.element);
        if (typeof node === 'string') throw refused(node);
        await page.send('DOM.focus', { backendNodeId: node });
      }
      if (command.clear) await page.send('Runtime.evaluate', { expression: CLEAR });
      await insertText(page, command.text);
      if (command.submit) {
        const enter = parseKey('Enter');
        if (typeof enter !== 'string') await pressKey(page, enter, 0);
      }
      await settle(context, tabId, command.submit ? 10_000 : 1_000);
      return report(context, tabId);
    }
    case 'press': {
      const stroke = parseKey(command.key);
      if (typeof stroke === 'string') throw refused(stroke);
      await guard(page, { ref: null, point: null }, 'press');
      for (let time = 0; time < Math.max(1, command.repeat); time += 1) {
        stopIfCancelled(context);
        await pressKey(page, stroke, command.hold_ms);
      }
      await settle(context, tabId, 5_000);
      return report(context, tabId);
    }
    case 'scroll': {
      const view = await viewport(context, tabId);
      const point =
        command.element !== null
          ? await pointOf(page, command.element)
          : { x: view.width / 2, y: view.height / 2 };
      const [dx, dy] = { up: [0, -1], down: [0, 1], left: [-1, 0], right: [1, 0] }[
        command.direction
      ];
      // An amount is screens: the tab's own width or height each.
      const step = (dx ?? 0) * view.width;
      const lift = (dy ?? 0) * view.height;
      await wheel(page, point, 1, step * command.amount, lift * command.amount);
      await settle(context, tabId, 1_000);
      return report(context, tabId);
    }
    case 'hover': {
      const point = await pointOf(page, command.element);
      await hoverAt(page, point, 1);
      await settle(context, tabId, 1_000);
      return report(context, tabId);
    }
    case 'select': {
      await guard(page, { ref: command.element, point: null }, 'select');
      const node = nodeOf(page, command.element);
      if (typeof node === 'string') throw refused(node);
      const { object } = await page.send<{ object?: { objectId?: string } }>('DOM.resolveNode', {
        backendNodeId: node,
      });
      if (object?.objectId === undefined)
        throw refused(`元素引用 ${command.element} 已失效；请重新 chrome.snapshot。`);
      const { result } = await page.send<{ result?: { value?: unknown } }>(
        'Runtime.callFunctionOn',
        {
          objectId: object.objectId,
          functionDeclaration: SELECT,
          arguments: [{ value: command.values }],
          returnByValue: true,
        },
      );
      if (result?.value === -1) throw refused(`${command.element} 不是下拉选择框（select）`);
      if (result?.value === 0) throw refused(`下拉框里没有这些选项：${command.values.join('、')}`);
      await settle(context, tabId, 1_000);
      return report(context, tabId);
    }
  }
}
