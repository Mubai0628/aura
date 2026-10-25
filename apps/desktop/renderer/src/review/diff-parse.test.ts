import { expect, test } from 'vitest';
import { changedWords, parseDiff, splitRows, wordDiff } from './diff-parse';

// The review page shows each change as lines with their numbers, side by
// side when asked, with the words that changed marked.

const patch = [
  'diff --git a/src/main.rs b/src/main.rs',
  'index 1111111..2222222 100644',
  '--- a/src/main.rs',
  '+++ b/src/main.rs',
  '@@ -1,4 +1,4 @@ fn main() {',
  ' fn main() {',
  '-    println!("hello");',
  '+    println!("hello, world");',
  '     let x = 1;',
  '-    x',
  '\\ No newline at end of file',
  '+    x + 1',
  '@@ -20,2 +20,3 @@',
  ' a',
  '+b',
  ' c',
  '',
].join('\n');

test('a diff becomes hunks of rows with the line numbers of both files', () => {
  const parsed = parseDiff(patch);
  expect(parsed.binary).toBe(false);
  expect(parsed.hunks.map((hunk) => hunk.header)).toEqual([
    '@@ -1,4 +1,4 @@ fn main() {',
    '@@ -20,2 +20,3 @@',
  ]);
  expect(parsed.hunks[0]!.rows).toEqual([
    { kind: 'context', text: 'fn main() {', old: 1, new: 1 },
    { kind: 'delete', text: '    println!("hello");', old: 2 },
    { kind: 'add', text: '    println!("hello, world");', new: 2 },
    { kind: 'context', text: '    let x = 1;', old: 3, new: 3 },
    { kind: 'delete', text: '    x', old: 4 },
    { kind: 'note', text: 'No newline at end of file' },
    { kind: 'add', text: '    x + 1', new: 4 },
  ]);
  expect(parsed.hunks[1]!.rows.map((row) => [row.old, row.new])).toEqual([
    [20, 20],
    [undefined, 21],
    [21, 22],
  ]);
});

test('new, deleted and binary files say so', () => {
  expect(parseDiff('diff --git a/x b/x\nnew file mode 100644\n@@ -0,0 +1 @@\n+a\n').added).toBe(
    true,
  );
  expect(parseDiff('diff --git a/x b/x\ndeleted file mode 100644\n').deleted).toBe(true);
  expect(parseDiff('diff --git a/x b/x\nBinary files a/x and b/x differ\n').binary).toBe(true);
});

test('side by side, a removed line sits beside the line that replaced it', () => {
  const rows = parseDiff(patch).hunks[0]!.rows;
  const split = splitRows(rows).map((row) => [row.left?.text, row.right?.text]);
  expect(split).toEqual([
    ['fn main() {', 'fn main() {'],
    ['    println!("hello");', '    println!("hello, world");'],
    ['    let x = 1;', '    let x = 1;'],
    ['    x', undefined],
    ['No newline at end of file', undefined],
    [undefined, '    x + 1'],
  ]);
});

test('the words that changed within a replaced line are marked on both sides', () => {
  expect(wordDiff('let total = a + b;', 'let sum = a + b + c;')).toEqual([
    [
      { text: 'let ', changed: false },
      { text: 'total', changed: true },
      { text: ' = a + b;', changed: false },
    ],
    [
      { text: 'let ', changed: false },
      { text: 'sum', changed: true },
      { text: ' = a + b', changed: false },
      { text: ' + c', changed: true },
      { text: ';', changed: false },
    ],
  ]);
  // Chinese text compares character by character.
  expect(wordDiff('保存设置', '保存全部设置')![1]).toEqual([
    { text: '保存', changed: false },
    { text: '全部', changed: true },
    { text: '设置', changed: false },
  ]);
  const rows = parseDiff(patch).hunks[0]!.rows;
  const words = changedWords(rows);
  expect(words.get(rows[1]!)).toEqual([{ text: '    println!("hello");', changed: false }]);
  expect(words.get(rows[2]!)).toEqual([
    { text: '    println!("hello', changed: false },
    { text: ', world', changed: true },
    { text: '");', changed: false },
  ]);
  expect(words.has(rows[0]!)).toBe(false);
});
