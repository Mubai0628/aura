// Acceptance provider: only the remote model service is substituted.
//
// No Core, permission, file or Renderer state is mocked. The provider is a real local HTTP
// server speaking the OpenAI chat-completions stream, and it drives a real `fs.write` Agent
// tool call into the production Core so the run reaches the real permission decision — which
// is the whole point: a ReadOnly session writing a workspace file must ask, and this provider
// is what makes the ask happen without a human typing.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';

const wait = (ms) => new Promise((done) => setTimeout(done, ms));

function currentState(body) {
  for (const message of [...body.messages].reverse()) {
    if (!['system', 'developer'].includes(message.role)) continue;
    const text = message.content
      .replaceAll('&lt;', '<')
      .replaceAll('&gt;', '>')
      .replaceAll('&amp;', '&');
    const start = text.indexOf('{"continuation"');
    for (let from = Math.max(0, start); from < text.length; from += 1) {
      if (text[from] !== '{') continue;
      let quoted = false;
      let depth = 0;
      for (let to = from; to < text.length; to += 1) {
        const char = text[to];
        if (quoted) {
          if (char === '\\') to += 1;
          else if (char === '"') quoted = false;
        } else if (char === '"') quoted = true;
        else if (char === '{') depth += 1;
        else if (char === '}' && --depth === 0) {
          const value = JSON.parse(text.slice(from, to + 1));
          if (value.turn) return value;
          break;
        }
      }
    }
  }
  throw new Error('Compiled current task state missing');
}

/**
 * Drives one real `fs.write` and then reports completion.
 *
 * `path` is workspace-relative. The write is declared as a create (`expected: absent`), which
 * is the ordinary "make this file" request and the one the dialog describes for a create.
 * A second provider request is answered with a plain final answer so the turn can finish and
 * the projection can be inspected.
 */
export async function writeProvider({
  path,
  text,
  observeRequest = null,
  planAction = null,
  maintenanceOutput = { schema_version: 1, candidates: [], events: [] },
  // The tool call's arguments arrive in `chunks` pieces `every` ms apart, as a model
  // writing a whole file into one call does; the final answer can be given.
  streamArguments = null,
  finalText = '已完成。',
} = {}) {
  assert.ok(path, 'writeProvider needs a workspace-relative path');
  const requests = [];
  const errors = [];
  const maintenanceRequests = [];
  const counts = new Map();
  let action = {
    tool: 'fs.write',
    arguments: { path, text, expected: { expected_kind: 'absent' } },
  };
  let hold;
  // The answer after a tool result can change between turns.
  let answer = finalText;
  // What the held answer has already said, and how long the next answer's headers wait.
  let holdBefore;
  let headersAfter = 0;
  const server = createServer((request, response) => {
    let raw = '';
    request.on('data', (part) => {
      raw += part;
    });
    request.on('end', () => {
      void respond().catch((error) => {
        errors.push(error.stack);
        response.destroy(error);
      });
    });
    async function respond() {
      const body = JSON.parse(raw);
      // Auxiliary requests have no tools/current-turn state. Keep them out of
      // the task's request counters and answer with a scripted Schema contract.
      if (
        !body.tools?.length &&
        body.messages?.some(
          (message) =>
            typeof message.content === 'string' &&
            message.content.includes('Return only JSON conforming to this schema (version 1)'),
        )
      ) {
        maintenanceRequests.push(body);
        response.writeHead(200, {
          'content-type': 'text/event-stream',
          'cache-control': 'no-cache',
        });
        response.end(
          `data: ${JSON.stringify({ id: 'l05-maintenance', choices: [{ index: 0, delta: { role: 'assistant', content: JSON.stringify(maintenanceOutput) }, finish_reason: 'stop' }], usage: { prompt_tokens: 20, completion_tokens: 10, total_tokens: 30 } })}\n\ndata: [DONE]\n\n`,
        );
        return;
      }
      observeRequest?.({ authorization: request.headers.authorization, body });
      const state = currentState(body);
      const turn = state.turn.turn_task_id;
      const step = counts.get(turn) ?? 0;
      counts.set(turn, step + 1);
      const id = `s06_${requests.length}`;
      const requestFact = { turn, step, tools: body.tools, messages: body.messages };
      requests.push(requestFact);
      const planned = planAction
        ? planAction(requestFact, action)
        : step === 0
          ? action
          : { tool: null, arguments: {} };
      if (headersAfter) {
        const delay = headersAfter;
        headersAfter = 0;
        await wait(delay);
      }
      response.writeHead(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' });
      const send = (delta, finish = null) =>
        response.write(
          `data: ${JSON.stringify({
            id,
            choices: [{ index: 0, delta, finish_reason: finish }],
            ...(finish
              ? { usage: { prompt_tokens: 1000, completion_tokens: 100, total_tokens: 1100 } }
              : {}),
          })}\n\n`,
        );
      send({ role: 'assistant' });
      if (step === 0 && hold) {
        const gate = hold;
        hold = undefined;
        if (holdBefore) send({ content: holdBefore });
        holdBefore = undefined;
        await gate;
      }
      await wait(200);
      if (planned.tool || step === 0) {
        if (!planned.tool) {
          send({ content: '模型生成正常结束。' }, 'stop');
          response.end('data: [DONE]\n\n');
          return;
        }
        const prefix = `${planned.tool.replaceAll('.', '_')}__`;
        const wire = body.tools.find((t) => t.function.name.startsWith(prefix));
        assert.ok(wire, `the real ${planned.tool} tool must be exposed to the model`);
        const call = {
          index: 0,
          id: `${id}_call_0`,
          type: 'function',
          function: {
            name: wire.function.name,
            arguments: JSON.stringify({
              ...planned.arguments,
              aura_purpose: '写入工作区文件以验证审批后的真实效果',
            }),
          },
        };
        send({ content: '准备写入。' });
        await wait(200);
        if (streamArguments) {
          const whole = call.function.arguments;
          const size = Math.ceil(whole.length / streamArguments.chunks);
          send({ tool_calls: [{ ...call, function: { ...call.function, arguments: '' } }] });
          for (let from = 0; from < whole.length; from += size) {
            await wait(streamArguments.every);
            send({
              tool_calls: [{ index: 0, function: { arguments: whole.slice(from, from + size) } }],
            });
          }
          send({}, 'tool_calls');
          response.end('data: [DONE]\n\n');
          return;
        }
        send({ tool_calls: [call] }, 'tool_calls');
        response.end('data: [DONE]\n\n');
        return;
      }
      // The tool result is in context now (approved and executed, or refused). Finish the turn.
      send({ content: answer });
      await wait(150);
      send({}, 'stop');
      response.end('data: [DONE]\n\n');
    }
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  const address = server.address();
  return {
    url: `http://127.0.0.1:${address.port}/v1`,
    requests,
    errors,
    maintenanceRequests,
    setAction: (tool, args) => {
      action = { tool, arguments: args };
    },
    // The next turn's answer waits until released; with `before`, after saying that much.
    holdNext: (before) => {
      holdBefore = before;
      let release;
      hold = new Promise((done) => {
        release = done;
      });
      return () => release();
    },
    // The next answer's headers come only after `ms`, as a gateway that waits for the
    // model before it answers at all.
    setFinalText: (text) => {
      answer = text;
    },
    delayHeadersNext: (ms) => {
      headersAfter = ms;
    },
    close: () => new Promise((done) => server.close(done)),
  };
}
