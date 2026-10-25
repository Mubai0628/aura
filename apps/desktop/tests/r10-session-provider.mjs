// Only the remote provider is substituted. No Core/Renderer state is mocked.
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { marker } from './r10-session-harness.mjs';

const wait = (ms) => new Promise((done) => setTimeout(done, ms));
export const reasoning = '先检查目录，再读取说明文件。';
export const answer = `已读取说明文件，校验标记为 ${marker}。`;

function currentState(body) {
  for (const message of [...body.messages].reverse()) {
    if (!['system', 'developer'].includes(message.role)) continue;
    const text = message.content
      .replaceAll('&lt;', '<')
      .replaceAll('&gt;', '>')
      .replaceAll('&amp;', '&');
    const start = text.indexOf('{"continuation"');
    // JSON map ordering is deterministic, but the parser accepts any first key.
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

export async function fixtureProvider({ noTool = false, long = true, observeRequest = null } = {}) {
  const requests = [];
  const errors = [];
  const counts = new Map();
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
      // An optional test observer validates the real wire without retaining authentication.
      observeRequest?.({ authorization: request.headers.authorization, body });
      const state = currentState(body);
      const turn = state.turn.turn_task_id;
      const step = counts.get(turn) ?? 0;
      counts.set(turn, step + 1);
      assert.ok(step < 3, 'unexpected repeated provider request');
      const id = `r10_${requests.length}`;
      const wire = (name) => {
        const found = body.tools.find((t) =>
          t.function.name.startsWith(`${name.replaceAll('.', '_')}__`),
        );
        assert.ok(found, `required real tool ${name}`);
        return found.function.name;
      };
      const call = (name, args, ordinal) => ({
        index: ordinal,
        id: `${id}_call_${ordinal}`,
        type: 'function',
        function: {
          name: wire(name),
          arguments: JSON.stringify({ ...args, aura_purpose: '验证用户请求的只读操作' }),
        },
      });
      requests.push({
        turn,
        step,
        stable: body.messages[0],
        tools: body.tools,
        markerInToolResult: body.messages.some(
          (m) => m.role === 'tool' && m.content.includes(marker),
        ),
      });
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
      await wait(1000);
      send({ reasoning_content: '先检查目录，' });
      await wait(600);
      send({ reasoning_content: '再读取说明文件。' });
      await wait(700);
      if (!noTool && step === 0) {
        send({ content: '正在读取' });
        await wait(450);
        send({ content: '工作区文件。' });
        await wait(450);
        const calls = [call('fs.list', { path: '.', limit: 100, cursor: null }, 0)];
        for (let n = 1; n <= (long ? 25 : 1); n += 1)
          calls.push(
            call(
              'fs.read',
              { path: 'note.txt', range: { range_kind: 'full' }, explicit_encoding: null },
              n,
            ),
          );
        send({ tool_calls: calls }, 'tool_calls');
      } else {
        if (!noTool)
          assert.ok(
            requests.at(-1).markerInToolResult,
            'actual file data must reach the next model step',
          );
        const lineage = state.lineages.find((l) => l.lineage_id === state.turn.lineage_id);
        assert.ok(lineage);
        send({ content: '读取完成，' });
        await wait(500);
        send({ content: '正在整理最终回复。' });
        await wait(500);
        send(
          {
            tool_calls: [
              call(
                'normal.complete',
                {
                  lineage_id: lineage.lineage_id,
                  expected_lineage_revision: lineage.revision,
                  final_report: noTool ? '你好，当前任务已完成。' : answer,
                  disclosed_limitations: [],
                },
                0,
              ),
            ],
          },
          'tool_calls',
        );
      }
      response.end('data: [DONE]\n\n');
    }
  });
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  return {
    url: `http://127.0.0.1:${server.address().port}/v1`,
    requests,
    errors,
    async close() {
      server.closeAllConnections();
      await new Promise((done) => server.close(done));
    },
  };
}
