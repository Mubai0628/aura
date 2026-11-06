// A fake chat platform that pushes its users' messages to the bot over a WebSocket, for trying a
// channel adapter. Only Node.js's own modules.
//
//   node fake-ws-platform.mjs [--port 8766]
//
// Prints `listening <port>` once it listens (port 0 picks a free one).
//
// The bot's side, at ws://127.0.0.1:<port>/gateway (JSON text frames):
// - the platform sends {"op": "hello"}; the bot answers {"op": "identify", "bot": "<a name>"} and
//   is sent {"op": "ready"};
// - each user message: {"op": "message", "d": {"id", "chat": {"type": "private" | "group",
//   "id"}, "author": {"id", "name"}, "content"}}. A message pushed while no bot is identified
//   waits for one; the same message pushed twice is delivered twice, as platforms redeliver;
// - the bot posts with {"op": "send", "d": {"chat": {"type", "id"}, "reply_to", "content"}};
// - the bot sets the command menu its users see with {"op": "commands", "d": {"commands":
//   [{"name", "description"}]}}, the whole menu each time.
//
// The tester's side (HTTP, JSON):
// - POST /push {"chat_type", "chat_id", "user_id", "user_name", "text", "id"}: a user's message
//   ("id" may be left out and is then numbered);
// - GET /sent: every message the bot posted, oldest first;
// - GET /menus: every command menu the bot set, oldest first;
// - GET /bots: how many bots are identified now.
import { createHash } from 'node:crypto';
import { createServer } from 'node:http';

const GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
const waiting = [];
const sent = [];
const menus = [];
const bots = new Set();
let pushed = 0;

/** One frame from the server: never masked, never fragmented. */
function frame(opcode, payload) {
  const length = payload.length;
  const head =
    length < 126
      ? Buffer.from([0x80 | opcode, length])
      : length < 65536
        ? Buffer.from([0x80 | opcode, 126, length >> 8, length & 0xff])
        : Buffer.concat([Buffer.from([0x80 | opcode, 127]), bigLength(length)]);
  return Buffer.concat([head, payload]);
}

function bigLength(length) {
  const bytes = Buffer.alloc(8);
  bytes.writeBigUInt64BE(BigInt(length));
  return bytes;
}

function say(socket, value) {
  if (!socket.destroyed) socket.write(frame(0x1, Buffer.from(JSON.stringify(value), 'utf8')));
}

function deliver(message) {
  if (bots.size === 0) {
    waiting.push(message);
    return;
  }
  for (const bot of bots) say(bot, { op: 'message', d: message });
}

/** Reads the bot's frames as they come: masked, maybe fragmented, with pings to answer. */
function reader(socket, onText) {
  let buffered = Buffer.alloc(0);
  let parts = [];
  return (chunk) => {
    buffered = Buffer.concat([buffered, chunk]);
    for (;;) {
      if (buffered.length < 2) return;
      const fin = (buffered[0] & 0x80) !== 0;
      const opcode = buffered[0] & 0x0f;
      const masked = (buffered[1] & 0x80) !== 0;
      let length = buffered[1] & 0x7f;
      let at = 2;
      if (length === 126) {
        if (buffered.length < 4) return;
        length = buffered.readUInt16BE(2);
        at = 4;
      } else if (length === 127) {
        if (buffered.length < 10) return;
        length = Number(buffered.readBigUInt64BE(2));
        at = 10;
      }
      const mask = masked ? buffered.subarray(at, at + 4) : undefined;
      if (masked) at += 4;
      if (buffered.length < at + length) return;
      const payload = Buffer.from(buffered.subarray(at, at + length));
      buffered = buffered.subarray(at + length);
      if (mask) for (let i = 0; i < payload.length; i += 1) payload[i] ^= mask[i % 4];
      if (opcode === 0x8) {
        socket.end(frame(0x8, Buffer.alloc(0)));
        return;
      }
      if (opcode === 0x9) {
        socket.write(frame(0xa, payload));
        continue;
      }
      if (opcode === 0x1 || opcode === 0x0) {
        parts.push(payload);
        if (fin) {
          const text = Buffer.concat(parts).toString('utf8');
          parts = [];
          onText(text);
        }
      }
    }
  };
}

function gateway(request, socket) {
  const key = request.headers['sec-websocket-key'];
  if (request.url !== '/gateway' || typeof key !== 'string') {
    socket.end('HTTP/1.1 400 Bad Request\r\n\r\n');
    return;
  }
  const accept = createHash('sha1')
    .update(key + GUID)
    .digest('base64');
  socket.write(
    'HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n' +
      `Sec-WebSocket-Accept: ${accept}\r\n\r\n`,
  );
  socket.setNoDelay(true);
  socket.on(
    'data',
    reader(socket, (text) => {
      let message;
      try {
        message = JSON.parse(text);
      } catch {
        return;
      }
      if (message.op === 'identify') {
        bots.add(socket);
        say(socket, { op: 'ready' });
        for (const queued of waiting.splice(0)) say(socket, { op: 'message', d: queued });
      } else if (message.op === 'send' && message.d && bots.has(socket)) {
        sent.push(message.d);
      } else if (message.op === 'commands' && message.d && bots.has(socket)) {
        menus.push(message.d);
      }
    }),
  );
  const gone = () => bots.delete(socket);
  socket.on('close', gone);
  socket.on('error', gone);
  say(socket, { op: 'hello' });
}

function body(request) {
  return new Promise((resolve, reject) => {
    let raw = '';
    request.setEncoding('utf8');
    request.on('data', (part) => {
      raw += part;
    });
    request.on('end', () => {
      try {
        resolve(raw ? JSON.parse(raw) : {});
      } catch (error) {
        reject(error);
      }
    });
  });
}

function json(response, status, value) {
  const text = JSON.stringify(value);
  response.writeHead(status, {
    'content-type': 'application/json; charset=utf-8',
    'content-length': Buffer.byteLength(text),
  });
  response.end(text);
}

const server = createServer((request, response) => {
  if (request.method === 'GET' && request.url === '/sent') return json(response, 200, { sent });
  if (request.method === 'GET' && request.url === '/menus') return json(response, 200, { menus });
  if (request.method === 'GET' && request.url === '/bots')
    return json(response, 200, { bots: bots.size });
  if (request.method === 'POST' && request.url === '/push') {
    body(request).then(
      (push) => {
        pushed += 1;
        const message = {
          id: push.id || `w-${pushed}`,
          chat: { type: push.chat_type || 'private', id: push.chat_id },
          author: { id: push.user_id, name: push.user_name || '' },
          content: push.text,
        };
        deliver(message);
        json(response, 200, { id: message.id });
      },
      () => json(response, 400, { error: 'not JSON' }),
    );
    return;
  }
  json(response, 404, { error: 'not found' });
});
server.on('upgrade', gateway);

const at = process.argv.indexOf('--port');
const port = at >= 0 ? Number(process.argv[at + 1]) : 8766;
server.listen(port, '127.0.0.1', () => {
  console.log(`listening ${server.address().port}`);
});
