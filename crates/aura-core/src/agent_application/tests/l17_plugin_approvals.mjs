import { createInterface } from 'node:readline';
const say = (message) => process.stdout.write(JSON.stringify(message) + '\n');
const log = (text) => say({ type: 'log', level: 'info', text });
const waiting = new Map();
const replies = new Map();
const approvals = new Map();
let next = 1;
const call = (method, params) =>
  new Promise((resolve) => {
    const id = next++;
    waiting.set(id, resolve);
    say({ type: 'request', id, method, params });
  });
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const queueOf = (map, key) => {
  if (!map.has(key)) map.set(key, { items: [], wakers: [] });
  return map.get(key);
};
const put = (map, key, item) => {
  const queue = queueOf(map, key);
  const waker = queue.wakers.shift();
  if (waker) waker(item);
  else queue.items.push(item);
};
const take = (map, key) => {
  const queue = queueOf(map, key);
  if (queue.items.length) return Promise.resolve(queue.items.shift());
  return new Promise((resolve) => queue.wakers.push(resolve));
};
const shown = (answer) =>
  JSON.stringify(answer.error ? { error: answer.error.code } : answer.result);
const names = { friend: '老王', buddy: '小李' };
// Sends `text` as `user` in their private chat, or in the group `team`; logs the answer, and the
// reply when one is owed.
async function send(label, user, text, { group = false, id = label, reply = true } = {}) {
  const chat = group ? { kind: 'group', id: 'team' } : { kind: 'private', id: user };
  log('send ' + label);
  const answer = await call('message.in', {
    channel: 'qq-main',
    chat,
    user: { id: user, name: names[user] },
    message_id: id,
    text,
  });
  log('answer ' + label + ' ' + shown(answer));
  if (reply && answer.result?.accepted && !answer.result.repeated) {
    const said = await take(replies, id);
    log('reply ' + label + ' ' + JSON.stringify(said));
  }
  return answer.result;
}
const handed = async (label, user) => {
  const approval = await take(approvals, user);
  log('approval ' + label + ' ' + JSON.stringify(approval));
};
// Until the conversation's turn waits for an approval, read as its panel would.
async function asking(session) {
  for (;;) {
    const state = await call('session.state', { session_id: session });
    if (state.result?.waiting_for === 'approval') return;
    await sleep(100);
  }
}
// Past the time a chat's decision waits for.
const LATER = 3200;
async function run() {
  await send('help', 'friend', '/help');
  await send('none', 'friend', '/approve');
  await send('stranger', 'stranger', '/approve');
  // friend's turn waits for its write, then for deleting the file for good.
  await send('write', 'friend', '写一个文件', { reply: false });
  await handed('first', 'friend');
  await send('soon', 'friend', '/approve');
  await sleep(LATER);
  await send('group', 'friend', '/approve', { group: true });
  await send('other', 'friend', '/approve 9');
  await send('approve', 'friend', '/approve 1');
  await send('approve-again', 'friend', '/approve 1', { id: 'approve' });
  await handed('second', 'friend');
  // buddy's turn, in a conversation of its own, waits behind friend's deletion.
  await send('buddy-new', 'buddy', '/new');
  const buddy = await send('buddy-write', 'buddy', '也写一个', { reply: false });
  await asking(buddy.session_id);
  await sleep(LATER);
  await send('approve-risky', 'friend', '/approve');
  // The deletion is refused in Aura's window; buddy's goes out after it.
  log('buddy-waiting {}');
  log('reply write ' + JSON.stringify(await take(replies, 'write')));
  await handed('third', 'buddy');
  await sleep(LATER);
  await send('buddy-session', 'buddy', '/approve-session');
  await handed('fourth', 'buddy');
  await sleep(LATER);
  await send('buddy-deny', 'buddy', '/deny 4');
  log('reply buddy-write ' + JSON.stringify(await take(replies, 'buddy-write')));
  // A wider permission mode, a while after the modes were listed.
  await send('wider-unlisted', 'friend', '/permission 3');
  await send('modes', 'friend', '/permission');
  await send('wider-soon', 'friend', '/permission 3');
  await sleep(LATER);
  await send('wider', 'friend', '/permission 3');
  log('done {}');
}
createInterface({ input: process.stdin }).on('line', (line) => {
  const message = JSON.parse(line);
  if (message.type === 'hello') {
    say({
      type: 'ready',
      protocol: 1,
      channels: [{ id: 'qq-main', max_text_chars: 2000, command_menu: true }],
    });
    run();
  } else if (message.type === 'response') {
    waiting.get(message.id)?.(message);
  } else if (message.type === 'reply') {
    put(replies, message.reply_to, message);
  } else if (message.type === 'approval') {
    log('got ' + message.chat.id + ' ' + message.approval.id);
    put(approvals, message.chat.id, message);
  } else if (message.type === 'commands') {
    log('menu ' + JSON.stringify(message));
  } else if (message.type === 'shutdown') {
    process.exit(0);
  }
});
