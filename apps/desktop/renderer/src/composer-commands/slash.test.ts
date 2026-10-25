import { expect, test } from 'vitest';
import type { CommandEntry } from '@aura/contracts/generated';
import {
  matchCommands,
  slashAllowedAt,
  slashQuery,
  typedSlash,
  withTemplate,
  withoutToken,
} from './slash';

const entry = (name: string, title: string): CommandEntry => ({
  name,
  title,
  description: '',
  kind: 'immediate',
  source: 'builtin',
  argument_hint: null,
  template: null,
  enabled: true,
  issue: null,
  location: null,
  revision: null,
});

test('only a typed slash counts; pasted, dropped, undone or composed text never does', () => {
  expect(typedSlash({ inputType: 'insertText', data: '/' })).toBe(true);
  for (const inputType of ['insertFromPaste', 'insertFromDrop', 'historyUndo', 'historyRedo'])
    expect(typedSlash({ inputType, data: '/' })).toBe(false);
  expect(typedSlash({ inputType: 'insertText', data: '、' })).toBe(false);
  expect(typedSlash({ inputType: 'insertText', data: '//' })).toBe(false);
});

test('the slash must start the text, a line, or follow whitespace', () => {
  expect(slashAllowedAt('', 0)).toBe(true);
  expect(slashAllowedAt('看一下 ', 4)).toBe(true);
  expect(slashAllowedAt('第一行\n', 4)).toBe(true);
  expect(slashAllowedAt('路径a', 3)).toBe(false);
  expect(slashAllowedAt('http:', 5)).toBe(false);
});

test('the query follows the token and ends at whitespace', () => {
  expect(slashQuery('/mod', 0, 4)).toBe('mod');
  expect(slashQuery('前 /', 2, 3)).toBe('');
  expect(slashQuery('/model 后面', 0, 9)).toBeUndefined();
  expect(slashQuery('mod', 0, 3)).toBeUndefined();
  expect(slashQuery('/mod', 0, 0)).toBeUndefined();
});

test('picking removes the token, or puts a template where it was', () => {
  expect(withoutToken('请 /comp 后文', 2, 7)).toEqual({ value: '请  后文', caret: 2 });
  expect(withTemplate('/rev', 0, 4, '请审查 $ARGUMENTS 的改动')).toEqual({
    value: '请审查  的改动',
    caret: 4,
  });
  expect(withTemplate('前 /x', 2, 4, '固定模板')).toEqual({ value: '前 固定模板', caret: 6 });
});

test('name prefixes come first, then names or titles containing the query', () => {
  const commands = [
    entry('memory', '记忆设置'),
    entry('model', '切换模型'),
    entry('diff', '审查改动'),
  ];
  expect(matchCommands(commands, 'm').map((c) => c.name)).toEqual(['memory', 'model']);
  expect(matchCommands(commands, 'od').map((c) => c.name)).toEqual(['model']);
  expect(matchCommands(commands, '审查').map((c) => c.name)).toEqual(['diff']);
  expect(matchCommands(commands, '').length).toBe(3);
});
