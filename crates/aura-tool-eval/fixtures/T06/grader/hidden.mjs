import test from 'node:test';
import assert from 'node:assert/strict';
import { execute, retryDecision, defaults, TransportFailure, HttpFailure, OutcomeUnknown } from './index.ts';

function fake(sequence) {
  let calls = 0;
  const sleeps = [];
  return {
    get calls() { return calls; },
    sleeps,
    dependencies: {
      async send() {
        const item = sequence[calls++];
        if (item instanceof Error) throw item;
        assert.ok(item, 'no accidental replay');
        return item;
      },
      async sleep(delay) { sleeps.push(delay); },
    },
  };
}

test('a post-send disconnect never replays any mutating method', async () => {
  for (const method of ['POST', 'PUT', 'DELETE']) {
    const transport = fake([new TransportFailure('after_send'), { status: 200, value: 'duplicate' }]);
    await assert.rejects(execute({ method, path: '/orders' }, transport.dependencies), (error) => {
      assert.ok(error instanceof OutcomeUnknown);
      assert.equal(error.code, 'outcome_unknown');
      assert.equal(error.attempts, 1);
      assert.match(error.message, /Inspect/);
      return true;
    });
    assert.equal(transport.calls, 1);
    assert.deepEqual(transport.sleeps, []);
  }
});

test('retry eligibility itself refuses ambiguous writes', () => {
  assert.equal(retryDecision('POST', new TransportFailure('after_send'), 1, defaults).retry, false);
  assert.equal(retryDecision('PUT', new HttpFailure(503), 1, defaults).retry, false);
});

test('a permanent post-send failure is still an unknown write outcome', async () => {
  const transport = fake([new TransportFailure('after_send', false)]);
  await assert.rejects(execute({ method: 'POST', path: '/orders' }, transport.dependencies), { code: 'outcome_unknown' });
  assert.equal(transport.calls, 1);
});

test('server failure after a write remains unknown and does not retry', async () => {
  const transport = fake([{ status: 503, value: '' }, { status: 200, value: 'duplicate' }]);
  await assert.rejects(execute({ method: 'DELETE', path: '/orders' }, transport.dependencies), { code: 'outcome_unknown', attempts: 1 });
  assert.equal(transport.calls, 1);
});

test('failure before sending a write can retry with bounded attempts', async () => {
  const transport = fake([new TransportFailure('before_send'), { status: 200, value: 'created' }]);
  assert.deepEqual(await execute({ method: 'POST', path: '/orders' }, transport.dependencies), { value: 'created', attempts: 2 });
  assert.deepEqual(transport.sleeps, [5]);
});

test('a read can retry after sending without losing its attempt count', async () => {
  const transport = fake([new TransportFailure('after_send'), { status: 502, value: '' }, { status: 200, value: 'read' }]);
  assert.deepEqual(await execute({ method: 'GET', path: '/orders' }, transport.dependencies), { value: 'read', attempts: 3 });
  assert.deepEqual(transport.sleeps, [5, 10]);
});

test('a definite throttle of a write can be retried', async () => {
  const transport = fake([{ status: 429, value: '', retry_after_ms: 3 }, { status: 201, value: 'created' }]);
  assert.equal((await execute({ method: 'POST', path: '/orders' }, transport.dependencies)).attempts, 2);
  assert.deepEqual(transport.sleeps, [3]);
});
