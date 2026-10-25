import test from 'node:test';
import assert from 'node:assert/strict';
import { execute, retryDecision, defaults, TransportFailure } from './index.ts';

test('retry eligibility rejects a possibly executed order', () => {
  assert.equal(retryDecision('POST', new TransportFailure('after_send'), 1, defaults).retry, false);
});

test('an order disconnect reports unknown outcome without a second send', async () => {
  let calls = 0;
  const dependencies = {
    async send() { calls += 1; throw new TransportFailure('after_send'); },
    async sleep() { assert.fail('an ambiguous write cannot sleep for a retry'); },
  };
  await assert.rejects(execute({ method: 'POST', path: '/orders' }, dependencies), { code: 'outcome_unknown', attempts: 1 });
  assert.equal(calls, 1);
});
