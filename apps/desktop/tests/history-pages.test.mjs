import assert from 'node:assert/strict';
import { test } from 'node:test';
import { loadTurns, resultValue } from './history-pages.mjs';

function historyApi(pages) {
  let calls = 0;
  return async () => {
    const n = ++calls;
    return {
      turns: [
        {
          turn: { turn_task_id: `turn-${n}` },
          user_messages: [],
          assistant_messages: [],
          next_messages: null,
        },
      ],
      next_before_turn_task_id: n < pages ? `cursor-${n}` : null,
    };
  };
}

test('thirty-nine pages that end on a null cursor are complete', async () => {
  const page = await loadTurns(historyApi(39), null, 'session');
  assert.equal(page.turnPages, 39);
  assert.equal(page.exhausted, true);
});

test('exactly forty pages that end on a null cursor are complete', async () => {
  const page = await loadTurns(historyApi(40), null, 'session');
  assert.equal(page.turnPages, 40);
  assert.equal(page.exhausted, true);
});

test('forty-one pages fail while a next cursor is still present', async () => {
  await assert.rejects(loadTurns(historyApi(41), null, 'session'), /cursor was exhausted/);
});

test('a repeated history cursor fails', async () => {
  let calls = 0;
  const api = async () => {
    calls += 1;
    return {
      turns: [
        {
          turn: { turn_task_id: `turn-${calls}` },
          user_messages: [],
          assistant_messages: [],
          next_messages: null,
        },
      ],
      next_before_turn_task_id: calls === 1 ? 'cursor-1' : 'cursor-1',
    };
  };
  await assert.rejects(loadTurns(api, null, 'session'), /did not advance|cursor repeated/);
});

test('message pages follow next_messages until it is null', async () => {
  let messages = 0;
  const api = async (_page, _method, request) => {
    if (request.within_turn) {
      messages += 1;
      return {
        turns: [
          {
            turn: { turn_task_id: 'turn-1' },
            user_messages: [{ event_id: `user-${messages}` }],
            assistant_messages: [],
            next_messages:
              messages < 2 ? { turn_task_id: 'turn-1', before_user_event_id: 'user-2' } : null,
          },
        ],
        next_before_turn_task_id: null,
      };
    }
    return {
      turns: [
        {
          turn: { turn_task_id: 'turn-1' },
          user_messages: [],
          assistant_messages: [],
          next_messages: { turn_task_id: 'turn-1', before_user_event_id: 'user-1' },
        },
      ],
      next_before_turn_task_id: null,
    };
  };
  const page = await loadTurns(api, null, 'session');
  assert.equal(page.messagePages, 2);
  assert.equal(page.exhausted, true);
  assert.equal(page.turns[0].user_messages.length, 2);
});

test('a message walk that stays open past the cap fails', async () => {
  const api = async (_page, _method, request) => {
    if (!request.within_turn) {
      return {
        turns: [
          {
            turn: { turn_task_id: 'turn-1' },
            user_messages: [],
            assistant_messages: [],
            next_messages: { turn_task_id: 'turn-1', before_user_event_id: 'start' },
          },
        ],
        next_before_turn_task_id: null,
      };
    }
    const previous = request.within_turn.before_user_event_id;
    return {
      turns: [
        {
          turn: { turn_task_id: 'turn-1' },
          user_messages: [],
          assistant_messages: [],
          next_messages: { turn_task_id: 'turn-1', before_user_event_id: `${previous}-next` },
        },
      ],
      next_before_turn_task_id: null,
    };
  };
  await assert.rejects(loadTurns(api, null, 'session'), /message page walk stopped/);
});

const succeeded = (value) => JSON.stringify({ outcome: { type: 'succeeded', payload: { value } } });

test('a result split on UTF-8 bytes is complete only when the byte length matches', async () => {
  const body = succeeded('你好');
  const split = Buffer.from(body, 'utf8');
  const pageSize = split.indexOf(Buffer.from('你', 'utf8')) + Buffer.byteLength('你', 'utf8');
  const api = async (_page, _method, request) => {
    const end = Math.min(request.offset + pageSize, split.length);
    const next = end >= split.length ? null : end;
    return {
      text: split.subarray(request.offset, end).toString('utf8'),
      next_offset: next,
      total_bytes: split.length,
    };
  };
  const result = await resultValue(api, null, 'session', 'call', 16_384);
  assert.equal(result.value, '你好');
});

test('a result cursor that remains after the character count looks finished still fails', async () => {
  const api = async () => ({ text: '{', next_offset: 3, total_bytes: 1 });
  await assert.rejects(resultValue(api, null, 'session', 'call', 16), /byte length was reached/);
});

test('forty-one result pages fail before JSON is parsed', async () => {
  let calls = 0;
  const api = async () => {
    calls += 1;
    return { text: 'x', next_offset: calls, total_bytes: 10_000 };
  };
  await assert.rejects(resultValue(api, null, 'session', 'call', 16), /body was exhausted/);
  assert.equal(calls, 40);
});

test('a repeated result offset fails before the body is parsed', async () => {
  const api = async () => ({ text: 'ab', next_offset: 2, total_bytes: 100 });
  await assert.rejects(
    resultValue(api, null, 'session', 'call', 16),
    /offset repeated before the body was complete/,
  );
});

test('a result page larger than the requested limit fails', async () => {
  const api = async () => ({ text: 'abcdef', next_offset: null, total_bytes: 6 });
  await assert.rejects(
    resultValue(api, null, 'session', 'call', 4),
    /exceeded the requested byte limit/,
  );
});
