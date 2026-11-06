// Aura 插件后台服务的帮助库，协议第 1 版。只用 Node.js 自带的模块（18 及以上），不用 npm 包。
//
// 复制进插件文件夹（例如 service/aura-plugin.mjs），在服务里：
//
//   import { AuraError, connect } from './aura-plugin.mjs';
//
//   const aura = await connect({ channels: [{ id: 'qq-main', max_text_chars: 2000, groups: true }] });
//   aura.onReply(async (reply) => { /* 发回平台 */ });
//   aura.onCommands(async (menu) => { /* 设置平台的命令菜单 */ });
//   aura.onApproval(async (asked) => { /* 发给那个私聊，或画成按钮 */ });
//   aura.onShutdown(async () => { /* 断开平台 */ });
//   // 收到平台的消息时：
//   const answer = await aura.messageIn({
//     channel: 'qq-main', chat: { kind: 'private', id: chatId }, user: { id: userId, name: '老王' },
//     message_id: messageId, text,
//   });
//   await aura.ended;   // Aura 让服务退出（或关了标准输入），退出的处理函数都跑完后
//
// connect() 之后 console.log、console.info、console.debug 写到标准错误（进插件日志），
// 标准输出只留给协议。处理函数一个接一个地跑；出错写进插件日志。处理函数登记之前收到的
// reply、commands、approval 先存着（每种最多 HELD_MAX 条，多了丢最早的），登记时按收到的顺序交给它。
import { createInterface } from 'node:readline';

export const PROTOCOL = 1;
export const MAX_LINE_BYTES = 1024 * 1024;
export const MAX_IN_FLIGHT = 16;
export const HELD_MAX = 100;

/** Aura 拒绝了一个请求：code 是固定的代码，reason 是给人看的原因。 */
export class AuraError extends Error {
  constructor(code, reason) {
    super(`${code}: ${reason}`);
    this.name = 'AuraError';
    this.code = code;
    this.reason = reason;
  }
}

const stopping = () => new AuraError('aura.stopping', 'Aura 已经让这个服务退出');

/**
 * 读 Aura 的 hello，回 ready，开始收发。
 * channels 每个渠道一项：id、max_text_chars，可选 markdown、buttons、command_menu、groups。
 */
export async function connect({
  channels = [],
  input = process.stdin,
  output = process.stdout,
} = {}) {
  const service = new AuraService(input, output);
  await service.start(channels);
  if (output === process.stdout) {
    for (const name of ['log', 'info', 'debug']) {
      console[name] = (...args) => console.error(...args);
    }
  }
  return service;
}

export class AuraService {
  #input;
  #output;
  #lines;
  #nextId = 0;
  #inFlight = 0;
  #pending = new Map();
  #waitingForSlot = [];
  #handlers = { reply: [], commands: [], approval: [] };
  #held = { reply: [], commands: [], approval: [] };
  #shutdownHandlers = [];
  #work = Promise.resolve();
  #helloSeen;
  #closed = false;
  #end;
  hello = undefined;

  constructor(input, output) {
    this.#input = input;
    this.#output = output;
    this.#lines = createInterface({ input, crlfDelay: Infinity });
    this.ended = new Promise((resolve) => {
      this.#end = resolve;
    });
    this.#helloSeen = new Promise((resolve, reject) => {
      this.#lines.on('line', (line) => this.#read(line, resolve, reject));
      this.#lines.on('close', () => {
        reject(new Error('Aura 没有发 hello 就关了标准输入'));
        this.#finish();
      });
    });
  }

  async start(channels) {
    const hello = await this.#helloSeen;
    if (hello.protocol !== PROTOCOL) {
      throw new Error(`这个帮助库只会协议第 ${PROTOCOL} 版，Aura 说的是 ${hello.protocol}`);
    }
    this.hello = hello;
    this.#send({ type: 'ready', protocol: PROTOCOL, channels });
    return hello;
  }

  // 设置

  get plugin() {
    return this.hello?.plugin;
  }

  /** 清单里的渠道：id、platform、title。 */
  get channels() {
    return [...(this.hello?.channels ?? [])];
  }

  /** 一个文字设置在 hello 时的值。 */
  setting(key, fallback = undefined) {
    return this.hello?.settings?.[key] ?? fallback;
  }

  /** 一个保密设置的值，没填时是 undefined。不要写进日志或文件。 */
  secret(key) {
    return process.env[`AURA_SECRET_${key.toUpperCase()}`];
  }

  /** 数据只写在这里，不要写进插件文件夹。 */
  get dataFolder() {
    return process.env.AURA_PLUGIN_DATA ?? process.cwd();
  }

  /** 回合结束后 Aura 要发回聊天的话：channel、chat、reply_to、text、format；Aura 按渠道的上限分了段时还有 part、parts。 */
  onReply(handler) {
    return this.#on('reply', handler);
  }

  /** 报了 command_menu 的渠道的整份命令菜单：channel、commands（每项 name、title、description，有参数的还有 argument_hint）。 */
  onCommands(handler) {
    return this.#on('commands', handler);
  }

  /**
   * 插件打开了远程批准时，交给私聊、等那个人决定的审批：channel、chat（kind、id）、approval（id、title、detail、decisions）、text。
   * 有按钮的平台按 decisions 画按钮，没有的把 text 发出去；用户的回复或按下的按钮当一条消息交给 messageIn。
   */
  onApproval(handler) {
    return this.#on('approval', handler);
  }

  onShutdown(handler) {
    this.#shutdownHandlers.push(handler);
    return handler;
  }

  // 请求

  /**
   * 调 Aura 的一个方法，等它回答。被拒时以 AuraError 失败；给了 timeoutMs，过了那么多毫秒以
   * name 为 TimeoutError 的错误失败。同时最多 16 个请求在等回答，多的在这里排队；超时的请求等到
   * 回答才让出空位。
   */
  async call(method, params = {}, { timeoutMs = undefined } = {}) {
    if (this.#closed) throw stopping();
    if (this.#inFlight < MAX_IN_FLIGHT) {
      this.#inFlight += 1;
    } else {
      // The slot is handed over by the answer that frees it.
      await new Promise((resolve) => this.#waitingForSlot.push(resolve));
      if (this.#closed) throw stopping();
    }
    this.#nextId += 1;
    const id = this.#nextId;
    const answered = new Promise((resolve) => this.#pending.set(id, resolve));
    try {
      this.#send({ type: 'request', id, method, params });
    } catch (error) {
      this.#pending.delete(id);
      this.#freeSlot();
      throw error;
    }
    const response = await within(answered, timeoutMs, method);
    if (!response) throw stopping();
    if (response.error) {
      throw new AuraError(response.error.code ?? 'aura.failed', response.error.message ?? '');
    }
    return response.result;
  }

  /** 把平台推来的一条用户消息交给 Aura：channel、chat（kind、id）、user（id、name）、message_id、text、reply_to。 */
  messageIn(message) {
    const params = {
      channel: message.channel,
      chat: { kind: message.chat.kind, id: String(message.chat.id) },
      user: { id: String(message.user.id) },
      message_id: String(message.message_id),
      text: message.text,
    };
    if (message.user.name) params.user.name = message.user.name;
    if (message.reply_to !== undefined && message.reply_to !== null) {
      params.reply_to = String(message.reply_to);
    }
    return this.call('message.in', params);
  }

  notify(text, tone = 'info') {
    return this.call('notify', { text, tone });
  }

  /** 状态栏的一条；不给 text 清掉。 */
  status(text = undefined, tone = 'info') {
    return this.call('status', text === undefined ? { tone } : { text, tone });
  }

  /** 文字设置现在的值。 */
  async settings() {
    return (await this.call('settings.get')).values;
  }

  /** 改一个文字设置，空字符串清掉，返回改后的文字设置。之后服务会带着新值重启。 */
  async setSetting(key, value) {
    return (await this.call('settings.set', { key, value })).values;
  }

  sessions() {
    return this.call('sessions.list');
  }

  sessionState(sessionId) {
    return this.call('session.state', { session_id: sessionId });
  }

  sessionEvents(sessionId, after = undefined) {
    const params = { session_id: sessionId };
    if (after !== undefined) params.after = after;
    return this.call('session.events', params);
  }

  send(sessionId, text, dedupeKey = undefined) {
    const params = { session_id: sessionId, text };
    if (dedupeKey !== undefined) params.dedupe_key = dedupeKey;
    return this.call('session.send', params);
  }

  /** 在会话里运行插件自己的一个提示命令（name 不带 /），作为插件的消息发出。 */
  runCommand(sessionId, name, args = '', dedupeKey = undefined) {
    const params = { session_id: sessionId, name };
    if (args) params.arguments = args;
    if (dedupeKey !== undefined) params.dedupe_key = dedupeKey;
    return this.call('command.run', params);
  }

  stop(sessionId) {
    return this.call('session.stop', { session_id: sessionId });
  }

  newSession({ workspace_id, title, dedupe_key } = {}) {
    const params = {};
    if (workspace_id !== undefined) params.workspace_id = workspace_id;
    if (title !== undefined) params.title = title;
    if (dedupe_key !== undefined) params.dedupe_key = dedupe_key;
    return this.call('session.new', params);
  }

  /** 写进插件日志：level 是 info、warning 或 error。Aura 不在时什么也不做。 */
  log(text, level = 'info') {
    try {
      this.#send({ type: 'log', level, text: String(text).slice(0, 4000) });
    } catch {
      // Aura is gone; there is no log to write to.
    }
  }

  // 收发

  #send(message) {
    const line = JSON.stringify(message);
    if (Buffer.byteLength(line, 'utf8') > MAX_LINE_BYTES) throw new Error('一行最多 1 MiB');
    this.#output.write(line + '\n');
  }

  #read(line, helloSeen, helloFailed) {
    let message;
    try {
      message = JSON.parse(line);
    } catch {
      return;
    }
    if (message === null || typeof message !== 'object') return;
    switch (message.type) {
      case 'hello':
        helloSeen(message);
        break;
      case 'response': {
        const resolve = this.#pending.get(message.id);
        if (!resolve) return;
        this.#pending.delete(message.id);
        this.#freeSlot();
        resolve(message);
        break;
      }
      case 'reply':
      case 'commands':
      case 'approval':
        this.#handOver(message.type, message);
        break;
      case 'error':
        console.error(`Aura：${message.code}：${message.message}`);
        break;
      case 'shutdown':
        helloFailed(new Error('Aura 让服务退出了'));
        this.#finish();
        break;
      default:
        break;
    }
  }

  #freeSlot() {
    const next = this.#waitingForSlot.shift();
    if (next) next();
    else this.#inFlight -= 1;
  }

  #on(kind, handler) {
    this.#handlers[kind].push(handler);
    for (const message of this.#held[kind].splice(0)) this.#later(this.#handlers[kind], message);
    return handler;
  }

  #handOver(kind, message) {
    if (this.#handlers[kind].length > 0) {
      this.#later(this.#handlers[kind], message);
      return;
    }
    const held = this.#held[kind];
    held.push(message);
    if (held.length > HELD_MAX) held.shift();
  }

  #later(handlers, ...args) {
    this.#work = this.#work.then(async () => {
      for (const handler of handlers) {
        try {
          await handler(...args);
        } catch (error) {
          this.log(`处理 Aura 发来的消息时出错：${error?.stack ?? error}`, 'error');
        }
      }
    });
    return this.#work;
  }

  #finish() {
    if (this.#closed) return;
    this.#closed = true;
    for (const resolve of this.#pending.values()) resolve(undefined);
    this.#pending.clear();
    for (const resolve of this.#waitingForSlot.splice(0)) resolve();
    this.#later(this.#shutdownHandlers).then(() => {
      // Standard input would keep the process running after Aura is done with it.
      if (this.#input === process.stdin) process.stdin.destroy();
      this.#end();
    });
  }
}

/** `answered`, or a TimeoutError once `timeoutMs` passed. */
async function within(answered, timeoutMs, method) {
  if (timeoutMs === undefined) return answered;
  let timer;
  const late = new Promise((_, reject) => {
    timer = setTimeout(() => {
      const error = new Error(`${method} 没有在 ${timeoutMs} 毫秒内得到回答`);
      error.name = 'TimeoutError';
      reject(error);
    }, timeoutMs);
  });
  try {
    return await Promise.race([answered, late]);
  } finally {
    clearTimeout(timer);
  }
}

/** 把一段话按平台一条消息的上限分成几段：先在空行处分，再在换行处分，最后硬切。 */
export function splitText(text, maxChars) {
  if (maxChars < 1) throw new Error('maxChars 至少是 1');
  const pieces = [];
  let rest = [...text];
  while (rest.length > maxChars) {
    const window = rest.slice(0, maxChars).join('');
    let cut = window.lastIndexOf('\n\n');
    if (cut <= 0) cut = window.lastIndexOf('\n');
    const take = cut <= 0 ? maxChars : [...window.slice(0, cut)].length;
    const piece = rest.slice(0, take).join('').replace(/\n+$/u, '');
    if (piece) pieces.push(piece);
    rest = [...rest.slice(take).join('').replace(/^\n+/u, '')];
  }
  if (rest.length > 0 || pieces.length === 0) pieces.push(rest.join(''));
  return pieces;
}
