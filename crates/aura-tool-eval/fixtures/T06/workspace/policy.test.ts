import { test } from 'node:test';
import assert from 'node:assert/strict';
import { defaults, HttpFailure, mutating, options, retryDecision } from './index.ts';

test('defaults are explicit', () => {
  assert.deepEqual(options(), defaults);
  assert.equal(defaults.max_attempts, 3);
});

test('invalid limits are refused', () => {
  for (const max_attempts of [0, 6, 1.5, NaN]) {
    assert.throws(() => options({ max_attempts }), RangeError);
  }
  assert.throws(() => options({ base_delay_ms: -1 }), RangeError);
});

test('only GET is considered nonmutating', () => {
  assert.equal(mutating('GET'), false);
  assert.equal(mutating('POST'), true);
  assert.equal(mutating('PUT'), true);
  assert.equal(mutating('DELETE'), true);
});

test('unknown exceptions do not retry', () => {
  assert.deepEqual(retryDecision('GET', new Error('bad caller'), 1, defaults), { retry: false, delay_ms: 0 });
});

test('delay does not become negative or infinite', () => {
  assert.equal(retryDecision('GET', new HttpFailure(429, -10), 1, defaults).delay_ms, 0);
  assert.equal(retryDecision('GET', new HttpFailure(429, Infinity), 1, defaults).delay_ms, 5);
});

test('an override does not mutate shared defaults', () => {
  const custom = options({ max_attempts: 1, base_delay_ms: 0 });
  assert.equal(custom.max_attempts, 1);
  assert.equal(custom.base_delay_ms, 0);
  assert.equal(defaults.max_attempts, 3);
  assert.equal(defaults.base_delay_ms, 5);
});

test('a delay cap applies to exponential backoff', () => {
  const settings = options({ base_delay_ms: 40, max_delay_ms: 50 });
  const first = retryDecision('GET', new HttpFailure(503), 1, settings);
  const second = retryDecision('GET', new HttpFailure(503), 2, settings);
  assert.equal(first.delay_ms, 40);
  assert.equal(second.delay_ms, 50);
});

test('the final permitted call cannot request another sleep', () => {
  const decision = retryDecision('GET', new HttpFailure(503), 3, defaults);
  assert.equal(decision.retry, false);
  assert.equal(decision.delay_ms, 0);
});
