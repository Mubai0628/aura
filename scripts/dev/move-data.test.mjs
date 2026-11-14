import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import { resolve } from 'node:path';
import { PassThrough } from 'node:stream';
import test from 'node:test';
import { moveRequestedData } from './move-data.mjs';

function core(lines, code) {
  const calls = [];
  const spawn = (file, args, options) => {
    calls.push({ file, args, options });
    const child = new EventEmitter();
    child.stdout = new PassThrough();
    child.stderr = new PassThrough();
    setImmediate(() => {
      for (const line of lines) child.stdout.write(`${line}\n`);
      child.stdout.end();
      child.stderr.end();
      setImmediate(() => child.emit('exit', code));
    });
    return child;
  };
  return { calls, spawn };
}

test('the development start runs the move with its own data folders and says nothing when none was asked', async () => {
  const printed = [];
  const { calls, spawn } = core(['{"outcome":"NothingAsked"}'], 0);
  const env = { LOCALAPPDATA: 'X:\\dev\\local', APPDATA: 'X:\\dev\\roaming' };
  const code = await moveRequestedData({
    root: 'X:\\Aura',
    env,
    spawn,
    print: (line) => printed.push(line),
  });
  assert.equal(code, 0);
  assert.deepEqual(printed, []);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].file, resolve('X:\\Aura', 'target/debug/aura-core.exe'));
  assert.deepEqual(calls[0].args, [
    '--move-data',
    `--component-manifest=${resolve('X:\\Aura', 'target/aura-dev-runtime/component-manifest.json')}`,
  ]);
  assert.equal(calls[0].options.env, env);
});

test('a move that happens or fails is shown, and the start goes on with its exit code', async () => {
  const printed = [];
  const lines = [
    '{"stage":"Copying","done":0,"total":10}',
    '{"outcome":"Failed { reason: \\"x\\" }"}',
  ];
  const { spawn } = core(lines, 3);
  const code = await moveRequestedData({
    root: 'X:\\Aura',
    env: {},
    spawn,
    print: (line) => printed.push(line),
  });
  assert.equal(code, 3);
  assert.deepEqual(
    printed,
    lines.map((line) => `[move] ${line}`),
  );
});
