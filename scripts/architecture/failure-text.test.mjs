import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
  failureTextProblems,
  readableFailure,
  unreadableFailures,
  withoutTests,
} from './failure-text.mjs';

test('a failure reads as a dotted code, a colon and a Chinese sentence', () => {
  assert.equal(readableFailure('skills.busy: Skill 正在被修改，没有执行；请稍后重试。'), true);
  assert.equal(readableFailure('{area}.interrupted: 这次操作在后台意外中断。'), true);
  assert.equal(readableFailure('{error}'), true, 'another error passed on is judged where written');
  for (const text of [
    'plugins.busy',
    'mcp.invalid_server: {reason}',
    'the Tool Activity cursor is unavailable',
    'busy: 没有点的代码',
    '{error:?}',
    // The window strips a code whose first part is letters only; this one would stay on screen.
    'p05.clean_acceptance.integrity_check_failed: 数据不是空白的，模型功能没有启动。',
  ])
    assert.equal(readableFailure(text), false, text);
});

test('the texts of bail, anyhow, ensure, error attributes and context are read', () => {
  const source = [
    'fn f() -> anyhow::Result<()> {',
    '    anyhow::bail!("plugins.busy");',
    '    let _ = anyhow!("mcp.not_stdio: 只有本地程序可以这样启动。");',
    '    ensure!(',
    '        ok(),',
    '        "the request is invalid"',
    '    );',
    '    ensure!(ok(), r#"a.b: 中文"#);',
    '    x.context("reading the file")?;',
    '    let _ = anyhow!(error);',
    '    Ok(())',
    '}',
    '#[derive(thiserror::Error)]',
    'enum E {',
    '    #[error("the cursor is unavailable")]',
    '    Cursor,',
    '    #[error(transparent)]',
    '    Other(#[from] std::io::Error),',
    '}',
  ].join('\n');
  assert.deepEqual(unreadableFailures(source), [
    [2, 'plugins.busy'],
    [4, 'the request is invalid'],
    [9, 'reading the file'],
    [15, 'the cursor is unavailable'],
  ]);
});

test('test code is left out, and what follows a test module declaration is still read', () => {
  const source = [
    '#[cfg(test)]',
    '#[path = "x_tests.rs"]',
    'mod tests;',
    'fn f() { anyhow::bail!("x.y"); }',
    '#[cfg(test)]',
    'mod more {',
    '    fn g() { anyhow::bail!("not counted {"); }',
    '}',
  ].join('\n');
  assert.equal(withoutTests(source).split('\n').length, 8);
  assert.deepEqual(unreadableFailures(source), [[4, 'x.y']]);
});

test('a failure text kept in a constant is read where it is used, or where it is written', () => {
  const source = [
    'const BUSY: &str = "skills.busy: Skill 正在被修改，没有执行；请稍后重试。";',
    'pub(crate) const BARE: &str = "plugins.busy";',
    'pub const ENGLISH: &str = "mcp.invalid: the server is invalid";',
    'const METHOD: &str = "packages.get";',
    'fn f() -> anyhow::Result<()> {',
    '    anyhow::bail!(BARE);',
    '    anyhow::ensure!(ok(), BUSY);',
    '    x.context(METHOD)?;',
    '    Err(anyhow::anyhow!(error))',
    '}',
  ].join('\n');
  assert.deepEqual(unreadableFailures(source), [
    [3, 'mcp.invalid: the server is invalid'],
    [6, 'plugins.busy'],
    [8, 'packages.get'],
  ]);
});

test('every text in a failures module is a readable failure', () => {
  const source = [
    'pub const A: &str = "a.b: 中文。";',
    'pub const B: &str = "a.c";',
    'pub(crate) const C: &str =',
    '    "plain words";',
    'pub const D: &str = r#"d.e: 中文"#;',
  ].join('\n');
  assert.deepEqual(unreadableFailures(source, { failuresModule: true }), [
    [2, 'a.c'],
    [3, 'plain words'],
  ]);
  assert.deepEqual(unreadableFailures(source), []);
});

test('a file may hold no more than its baseline, and a lower count lowers the baseline', () => {
  assert.deepEqual(failureTextProblems({ 'a.rs': 2 }, { 'a.rs': 2 }), []);
  assert.equal(failureTextProblems({ 'a.rs': 3 }, { 'a.rs': 2 }).length, 1);
  assert.equal(failureTextProblems({ 'new.rs': 1 }, {}).length, 1);
  assert.match(failureTextProblems({}, { 'a.rs': 1 })[0], /lower its count/u);
});
