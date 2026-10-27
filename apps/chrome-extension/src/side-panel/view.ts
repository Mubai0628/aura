/**
 * The side panel, drawn from the service worker's state: the connection, pairing, sharing the
 * current tab while it is not shared, the shared tabs with their state and a stop each, a tab
 * handed to the user, and stopping every share. Only states and button words; nothing explains
 * itself in prose.
 */
import type { ConnectionState } from '../native.js';
import type { PanelRequest, PanelState } from '../panel-messages.js';
import type { PairRefusal, ShareScope, ShareView } from '../protocol.js';

export interface CurrentTab {
  tab_id: number;
  window_id: number;
  url: string;
  title: string;
}

/** What the user has typed or chosen, kept across redraws. */
export interface Form {
  code: string;
  session: string;
  scope: ShareScope;
}

export interface ViewModel {
  state: PanelState;
  tab: CurrentTab | null;
  form: Form;
  now: number;
}

export interface Actions {
  send(request: PanelRequest): void;
  edit(change: Partial<Form>): void;
}

const STATUS: Record<ConnectionState['kind'], string> = {
  connecting: '连接中',
  not_registered: 'Aura 未开启 Chrome 控制',
  not_running: 'Aura 未运行',
  pairing_required: '未配对',
  refused: '连接被拒绝',
  connected: '已连接',
};

const PAIR_REFUSAL: Record<PairRefusal, string> = {
  code_invalid: '配对码不对',
  code_expired: '配对码已过期',
  too_many_attempts: '错误次数过多，请在 Aura 里重新获取配对码',
  extension_not_allowed: 'Aura 不接受这个扩展',
  protocol_unsupported: 'Aura 与扩展版本不匹配',
};

const SHARE_STATE: Record<ShareView['state'], string> = {
  active: '进行中',
  awaiting_origin: '等待确认新网站',
  with_user: '你在操作',
};

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text = '',
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text) node.textContent = text;
  return node;
}

function button(label: string, onClick: () => void, className = 'button'): HTMLButtonElement {
  const node = element('button', className, label);
  node.type = 'button';
  node.addEventListener('click', onClick);
  return node;
}

/** Whether Aura can be given this tab: an ordinary web page. */
export function shareable(url: string): boolean {
  if (!/^https?:\/\//iu.test(url)) return false;
  const host =
    url
      .replace(/^https?:\/\//iu, '')
      .split(/[/?#:]/u)[0]
      ?.toLowerCase() ?? '';
  return host !== 'chrome.google.com' && host !== 'chromewebstore.google.com';
}

function remaining(deadline: number, now: number): string {
  const seconds = Math.max(0, Math.ceil((deadline - now) / 1000));
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, '0')}`;
}

function pairing(model: ViewModel, actions: Actions): HTMLElement {
  const section = element('section', 'pairing');
  const input = element('input', 'code');
  input.inputMode = 'numeric';
  input.maxLength = 8;
  input.placeholder = '配对码';
  input.value = model.form.code;
  input.addEventListener('input', () => actions.edit({ code: input.value }));
  section.append(
    input,
    button('配对', () => actions.send({ type: 'pair', code: input.value })),
  );
  const { connection } = model.state;
  if (connection.kind === 'pairing_required' && connection.refused !== null)
    section.append(element('p', 'problem', PAIR_REFUSAL[connection.refused]));
  return section;
}

/** Sharing the current tab; none for a tab already shared, which the list below shows. */
function currentTab(model: ViewModel, actions: Actions): HTMLElement | null {
  const { tab, state, form } = model;
  if (tab === null || state.shares.some((share) => share.tab_id === tab.tab_id)) return null;
  const section = element('section', 'current');
  section.append(element('h2', 'tab-title', tab.title || tab.url));
  if (!shareable(tab.url)) {
    section.append(element('p', 'muted', '此页面不能共享'));
    return section;
  }
  const sessions = element('select', 'sessions');
  for (const choice of state.sessions) {
    const option = element('option', '', `${choice.title} · ${choice.workspace}`);
    option.value = choice.session_id;
    sessions.append(option);
  }
  const chosen = state.sessions.some((choice) => choice.session_id === form.session)
    ? form.session
    : (state.sessions[0]?.session_id ?? '');
  sessions.value = chosen;
  sessions.addEventListener('change', () => actions.edit({ session: sessions.value }));
  const scope = element('select', 'scope');
  for (const [value, label] of [
    ['session', '本会话'],
    ['turn', '本轮'],
  ] as const) {
    const option = element('option', '', label);
    option.value = value;
    scope.append(option);
  }
  scope.value = form.scope;
  scope.addEventListener('change', () => actions.edit({ scope: scope.value as ShareScope }));
  const share_ = button(
    '共享给 Aura',
    () =>
      actions.send({
        type: 'share',
        tab_id: tab.tab_id,
        window_id: tab.window_id,
        url: tab.url,
        title: tab.title,
        session_id: sessions.value,
        scope: scope.value as ShareScope,
      }),
    'button primary',
  );
  share_.disabled = chosen === '';
  section.append(sessions, scope, share_);
  if (state.share_refused !== null) section.append(element('p', 'problem', state.share_refused));
  return section;
}

function sharedList(model: ViewModel, actions: Actions): HTMLElement {
  const list = element('ul', 'shares');
  for (const share of model.state.shares) {
    const item = element('li', `share ${share.state}`);
    item.append(
      element('span', 'share-session', share.session_title),
      element('span', 'share-origin', share.origin),
      element('span', 'share-state', SHARE_STATE[share.state]),
    );
    if (share.state === 'awaiting_origin')
      item.append(
        button('允许', () =>
          actions.send({
            type: 'answer_origin',
            tab_id: share.tab_id,
            origin: share.origin,
            allow: true,
          }),
        ),
        button('停止共享', () =>
          actions.send({
            type: 'answer_origin',
            tab_id: share.tab_id,
            origin: share.origin,
            allow: false,
          }),
        ),
      );
    else
      item.append(
        button('停止共享', () => actions.send({ type: 'unshare', tab_id: share.tab_id })),
      );
    list.append(item);
  }
  return list;
}

function assistBanner(model: ViewModel, actions: Actions): HTMLElement | null {
  const assist = model.state.assist;
  if (assist === null) return null;
  const banner = element('section', 'assist');
  banner.append(
    element('p', 'assist-reason', assist.reason),
    element('p', 'assist-time', remaining(assist.deadline, model.now)),
    button('完成', () => actions.send({ type: 'assist', outcome: 'done' }), 'button primary'),
    button('取消', () => actions.send({ type: 'assist', outcome: 'declined' })),
  );
  return banner;
}

export function render(root: HTMLElement, model: ViewModel, actions: Actions): void {
  const { connection } = model.state;
  const header = element('header', `status ${connection.kind}`);
  header.append(element('span', 'status-label', STATUS[connection.kind]));
  const parts: HTMLElement[] = [header];
  if (connection.kind === 'not_running' || connection.kind === 'not_registered')
    parts.push(button('重新连接', () => actions.send({ type: 'reconnect' })));
  if (connection.kind === 'pairing_required') parts.push(pairing(model, actions));
  if (connection.kind === 'connected') {
    const banner = assistBanner(model, actions);
    if (banner !== null) parts.push(banner);
    const current = currentTab(model, actions);
    if (current !== null) parts.push(current);
    if (model.state.shares.length > 0) {
      parts.push(sharedList(model, actions));
      parts.push(button('全部停止', () => actions.send({ type: 'stop' }), 'button stop'));
    }
  }
  root.replaceChildren(...parts);
}
