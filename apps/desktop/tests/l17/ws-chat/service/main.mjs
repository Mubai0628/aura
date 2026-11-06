// 假 WebSocket 平台的渠道适配：平台推来的每条消息交给 Aura，把 Aura 的回复发回去，Aura 的命令
// 菜单设成平台的菜单；连没连上写在 Aura 的状态栏。断开后等一会儿重连，越等越久，最长 60 秒。
import { AuraError, connect } from './aura-plugin.mjs';

const CHANNEL = 'ws-main';
const aura = await connect({
  channels: [{ id: CHANNEL, max_text_chars: 1000, groups: true, command_menu: true }],
});
const address = aura.setting('address', '');
let socket;
let identified = false;
let leaving = false;
let wait = 1000;
// Aura sends the menu once it is ready and again only when it changes: the newest one is kept
// for each connection to the platform.
let menu;

const setMenu = () => {
  if (!menu || !identified || socket?.readyState !== WebSocket.OPEN) return;
  const commands = menu.commands.map((command) => ({
    name: command.name,
    description: command.title,
  }));
  socket.send(JSON.stringify({ op: 'commands', d: { commands } }));
};

aura.onCommands((sent) => {
  menu = sent;
  setMenu();
});

const post = (chat, text, replyTo) => {
  if (socket?.readyState !== WebSocket.OPEN) {
    aura.log('没有连上假平台，消息没有发出去', 'warning');
    return;
  }
  const d = { chat: { type: chat.kind, id: chat.id }, reply_to: replyTo, content: text };
  socket.send(JSON.stringify({ op: 'send', d }));
};

aura.onReply((reply) => post(reply.chat, reply.text, reply.reply_to));
aura.onShutdown(() => {
  leaving = true;
  socket?.close();
});

async function received(message) {
  if (message.op === 'hello') {
    socket.send(JSON.stringify({ op: 'identify', bot: 'aura' }));
    return;
  }
  if (message.op === 'ready') {
    wait = 1000;
    identified = true;
    setMenu();
    void aura.status('已连上假平台', 'success').catch(() => undefined);
    return;
  }
  if (message.op !== 'message') return;
  const { id, chat, author, content } = message.d;
  try {
    const answer = await aura.messageIn({
      channel: CHANNEL,
      chat: { kind: chat.type, id: chat.id },
      user: author.name ? { id: author.id, name: author.name } : { id: author.id },
      message_id: id,
      text: content,
    });
    aura.log(`消息 ${id}：${JSON.stringify(answer)}`);
  } catch (error) {
    if (!(error instanceof AuraError)) throw error;
    aura.log(`Aura 没有收下消息 ${id}：${error.code} ${error.reason}`, 'warning');
  }
}

function open() {
  const next = new WebSocket(address);
  socket = next;
  identified = false;
  next.onmessage = (event) => {
    let message;
    try {
      message = JSON.parse(String(event.data));
    } catch {
      aura.log('假平台发来的不是 JSON', 'warning');
      return;
    }
    void received(message);
  };
  next.onclose = () => {
    if (leaving || socket !== next) return;
    aura.log(`和假平台断开了，${wait / 1000} 秒后重连`, 'warning');
    void aura.status('没连上假平台，等着重连', 'warning').catch(() => undefined);
    setTimeout(open, wait);
    wait = Math.min(wait * 2, 60_000);
  };
}

if (address) open();
else aura.log('没有填假平台的地址', 'error');
await aura.ended;
