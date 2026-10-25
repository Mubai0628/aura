import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execute, readOrders, submitOrder, TransportFailure } from './index.ts';
import type { Dependencies, Response } from './types.ts';

function scripted(sequence: Array<Response | Error>) {
  let attempts = 0;
  const sleeps: number[] = [];
  const dependencies: Dependencies = {
    async send() {
      const next = sequence[attempts++];
      if (next instanceof Error) throw next;
      assert.ok(next, 'the client cannot send past the script');
      return next;
    },
    async sleep(delay) {
      sleeps.push(delay);
    },
  };
  return { dependencies, sleeps, count: () => attempts };
}

test('an order response carries the attempt count', async () => {
  const fake = scripted([{ status: 201, value: 'order-1' }]);
  assert.deepEqual(await submitOrder('{}', fake.dependencies), { value: 'order-1', attempts: 1 });
  assert.equal(fake.count(), 1);
  assert.deepEqual(fake.sleeps, []);
});

test('reads retry temporary connection errors', async () => {
  const fake = scripted([new TransportFailure('before_send'), { status: 200, value: '[]' }]);
  assert.deepEqual(await readOrders(fake.dependencies), { value: '[]', attempts: 2 });
  assert.deepEqual(fake.sleeps, [5]);
});

test('a definite throttle uses its bounded delay', async () => {
  const fake = scripted([{ status: 429, value: '', retry_after_ms: 10000 }, { status: 200, value: 'ok' }]);
  const result = await readOrders(fake.dependencies);
  assert.equal(result.attempts, 2);
  assert.deepEqual(fake.sleeps, [1000]);
});

test('ordinary client errors do not retry', async () => {
  const fake = scripted([{ status: 404, value: '' }]);
  await assert.rejects(readOrders(fake.dependencies), { code: 'http_failed', status: 404 });
  assert.equal(fake.count(), 1);
});

test('the last permitted attempt can succeed', async () => {
  const fake = scripted([new TransportFailure('before_send'), new TransportFailure('before_send'), { status: 200, value: 'ok' }]);
  assert.equal((await readOrders(fake.dependencies)).attempts, 3);
  assert.deepEqual(fake.sleeps, [5, 10]);
});

test('invalid paths do not reach the transport', async () => {
  const fake = scripted([]);
  await assert.rejects(execute({ method: 'GET', path: '//another-service' }, fake.dependencies), /relative path/);
  assert.equal(fake.count(), 0);
});

test('permanent failures are preserved', async () => {
  const failure = new TransportFailure('before_send', false);
  const fake = scripted([failure]);
  await assert.rejects(readOrders(fake.dependencies), (error) => error === failure);
  assert.equal(fake.count(), 1);
});

test('attempt exhaustion does not sleep after the last call', async () => {
  const fake = scripted([new TransportFailure('before_send'), new TransportFailure('before_send')]);
  await assert.rejects(execute({ method: 'GET', path: '/orders' }, fake.dependencies, { max_attempts: 2 }), { code: 'transport_failed' });
  assert.equal(fake.count(), 2);
  assert.deepEqual(fake.sleeps, [5]);
});
