import assert from 'node:assert/strict';
import test from 'node:test';
import { RULES } from './comment-hygiene.mjs';

const kinds = (text) => RULES.filter(([, rule]) => rule.test(text)).map(([kind]) => kind);

test('a date is a process note, also where it ends a sentence', () => {
  assert.deepEqual(kinds('// The limit was set on 2026-10-13.'), ['date']);
  assert.deepEqual(kinds('// measured on 10-13, before the change'), ['date']);
  assert.deepEqual(kinds('// writes runs/2026-10-13.log'), []);
});

test('stage labels and people are process notes; line anchors and Claude Code are not', () => {
  assert.deepEqual(kinds('// L13.4: inspect elements'), ['stage']);
  assert.deepEqual(kinds('// see release.md#L12'), []);
  assert.deepEqual(kinds('// the plugin format of Claude Code'), []);
  assert.deepEqual(kinds('// the owner decision was'), ['person']);
});
