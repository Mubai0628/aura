import assert from 'node:assert/strict';
import test from 'node:test';
import { page } from './page.ts';
test('cursor and boundaries', () => {
  assert.deepEqual(page([1, 2, 3, 4], 2, 2), [3, 4]);
  assert.deepEqual(page([1, 2], 7, 1), []);
  assert.deepEqual(page([1], 0, 0), []);
  const input = [1, 2, 3];
  assert.deepEqual(page(input, 1, 8), [2, 3]);
  assert.deepEqual(input, [1, 2, 3]);
});
